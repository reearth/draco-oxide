//! Attribute decoding: framing (encoder count, domains, descriptors) and the
//! driver that sequences, reverses prediction, inverts transforms, and hands back
//! portable (quantized-integer) attributes.

mod ds;
mod inverse_transform;
mod prediction;
mod sequence;
mod sequential;

#[cfg(feature = "dequantize")]
pub(crate) mod dequantize;

use crate::connectivity::{Connectivity, EdgebreakerConnectivity};
use crate::entropy::{rans::RansSymbolDecoder, start_symbol_decoder, unzigzag, AnySymbolDecoder};
use crate::{AttributeTransform, Err};
use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::attribute::prediction_scheme::mesh_normal_prediction::{
    accumulate_face_normal_sums, canonical_normal_to_oct, sum_to_canonical_normal,
};
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::attribute::sequence::Traverser;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::mesh::ds::{GenericAttributeDs, GenericCornerTable, IdentityDS};
use draco_oxide_core::types::{
    AttributeValueIdx, CornerIdx, NdVector, PointIdx, VecPointIdx, Vector, VertexIdx,
};
use draco_oxide_core::utils::bit_coder::leb128_read;
use ds::{build_attribute_ds, build_ds, GeneralDs, Input};

use inverse_transform::InverseTransform;
use prediction::Predictor;

/// The portabilization type ids on the wire.
const PORT_GENERIC: u8 = 0;
const PORT_TO_BITS: u8 = 1;
const PORT_QUANTIZATION_COORDINATE_WISE: u8 = 2;
const PORT_OCTAHEDRAL: u8 = 3;

/// The attribute-data id (`-1` on the wire) an attribute carries when it rides
/// the position connectivity rather than a seam stream of its own.
const NO_ATTRIBUTE_DATA: u8 = 0xFF;

/// The decoded attribute section: the point-indexed faces plus, per attribute,
/// the portable integer attribute and its dequantization parameters.
pub(crate) struct DecodedAttributes {
    pub faces: Vec<[PointIdx; 3]>,
    pub attributes: Vec<Attribute>,
    pub transforms: Vec<AttributeTransform>,
}

/// One attribute's wire descriptor.
struct Descriptor {
    /// Index of the attribute connectivity this attribute rides, into the
    /// connectivity section's seam streams. [`NO_ATTRIBUTE_DATA`] means it
    /// rides the position connectivity instead.
    att_data_id: u8,
    att_type: AttributeType,
    /// The declared component type. Only the generic encoder keeps values in
    /// it; the other encoders force the portable representation to I32.
    component_type: ComponentDataType,
    num_components: usize,
    uid: u32,
    port_type: u8,
    domain: AttributeDomain,
}

impl Descriptor {
    /// Whether this attribute can be another attribute's prediction parent.
    /// The generic encoder builds no portable representation, so an attribute
    /// it carries is never available to predict from.
    fn is_prediction_parent(&self) -> bool {
        self.att_type == AttributeType::Position && self.port_type != PORT_GENERIC
    }

    /// The component count of the portable (quantized-integer) representation:
    /// octahedral attributes travel as 2-component values regardless of their
    /// original dimension.
    fn portable_num_components(&self) -> usize {
        if self.port_type == PORT_OCTAHEDRAL {
            2
        } else {
            self.num_components
        }
    }
}

/// Decodes the whole attribute section, positioned right after connectivity.
pub(crate) fn decode_attributes(
    reader: &mut Reader<'_>,
    conn: &Connectivity,
) -> Result<DecodedAttributes, Err> {
    match conn {
        Connectivity::Edgebreaker(conn) => decode_traversed_attributes(reader, conn),
        Connectivity::Sequential(conn) => sequential::decode_attributes(reader, conn),
    }
}

/// Decodes the attribute section of an edgebreaker stream, where each attribute
/// is sequenced over a traversal of its own connectivity.
fn decode_traversed_attributes(
    reader: &mut Reader<'_>,
    conn: &EdgebreakerConnectivity,
) -> Result<DecodedAttributes, Err> {
    let num_atts = reader.read_u8()? as usize;

    // Framing part 1: per-attribute connectivity id, domain, and traversal method.
    let mut headers = Vec::with_capacity(num_atts);
    for _ in 0..num_atts {
        let att_data_id = reader.read_u8()?;
        let domain = AttributeDomain::read_from(reader)?;
        let traversal = reader.read_u8()?;
        if traversal != 0 {
            return Err(Err::Unimplemented);
        }
        headers.push((att_data_id, domain));
    }

    // Framing part 2: per-attribute descriptors.
    let mut descriptors = Vec::with_capacity(num_atts);
    for (att_data_id, domain) in headers {
        let atts_in_encoder = reader.read_u8()?;
        if atts_in_encoder != 1 {
            return Err(Err::MalformedAttribute(
                "each attribute encoder must carry exactly one attribute",
            ));
        }
        let att_type = AttributeType::read_from(reader)?;
        let component_type = ComponentDataType::read_from(reader)?;
        let num_components = reader.read_u8()? as usize;
        let _normalized = reader.read_u8()?;
        let uid = leb128_read(reader)? as u32;
        let port_type = reader.read_u8()?;
        descriptors.push(Descriptor {
            att_data_id,
            att_type,
            component_type,
            num_components,
            uid,
            port_type,
            domain,
        });
    }

    let num_corners = conn.num_faces * 3;

    // Per-attribute seam edges, in attribute order. An attribute names its seam
    // stream by `att_data_id`, which is independent of the order the descriptors
    // arrive in. An attribute on the position domain rides the position
    // connectivity whatever its id, and carries no encoded seams: boundary edges
    // (its only seams) are already seams for every attribute through the corner
    // table itself.
    let mut seams: Vec<Vec<bool>> = Vec::with_capacity(num_atts);
    for desc in &descriptors {
        if desc.domain == AttributeDomain::Corner && desc.att_data_id != NO_ATTRIBUTE_DATA {
            let s = conn.attribute_seams.get(desc.att_data_id as usize).ok_or(
                Err::MalformedAttribute(
                    "attribute names a seam stream the connectivity did not encode",
                ),
            )?;
            seams.push(s.clone());
        } else {
            seams.push(vec![false; num_corners]);
        }
    }

    let seam_sets: Vec<&[bool]> = seams.iter().map(|s| s.as_slice()).collect();
    let seeds = sequence::traversal_seeds(conn.num_faces);

    // The placeholder attributes the decoded payloads replace, one per attribute
    // in descriptor order.
    let placeholders: Vec<Attribute> = descriptors
        .iter()
        .map(|desc| {
            Attribute::new_empty(
                AttributeId::new(desc.uid as usize),
                desc.att_type,
                desc.domain,
                ComponentDataType::I32,
                desc.portable_num_components(),
            )
        })
        .collect();

    // When no attribute carries an interior seam, points coincide with position
    // vertices for every attribute: the whole point/seam layer of `AttributeDS`
    // would be an identity map. Take the identity fast path, which reuses the
    // connectivity the reconstruction already produced instead of rebuilding a
    // point data structure and per-attribute corner tables.
    let any_interior_seam = seam_sets.iter().any(|s| {
        s.iter()
            .enumerate()
            .any(|(c, &b)| b && conn.corner_table.opposite(CornerIdx::from(c)).is_some())
    });

    let (faces, attributes, transforms) = if any_interior_seam {
        // General path: build the refined point space and each attribute's
        // sector decomposition.
        let fan_input = Input {
            pos_ct: &conn.corner_table,
            corner_to_vertex: &conn.corner_to_vertex,
            vertex_corners: &conn.vertex_corners,
            is_vert_hole: &conn.is_vert_hole,
            num_vertices: conn.num_vertices,
            num_corners,
        };
        let (ds, fans) = build_ds(fan_input, &seam_sets);
        let faces: Vec<[PointIdx; 3]> = (0..conn.num_faces)
            .map(|f| {
                [
                    ds.point_idx(CornerIdx::from(3 * f)),
                    ds.point_idx(CornerIdx::from(3 * f + 1)),
                    ds.point_idx(CornerIdx::from(3 * f + 2)),
                ]
            })
            .collect();
        // Attributes with identical seam sets get identical sector
        // decompositions and hence identical traversals, so they share one
        // decode walk. Each group id is its smallest member index.
        let mut group_ids: Vec<usize> = Vec::with_capacity(num_atts);
        for i in 0..num_atts {
            let gid = (0..i)
                .find(|&j| seams[j] == seams[i])
                .map(|j| group_ids[j])
                .unwrap_or(i);
            group_ids.push(gid);
        }
        let adss = build_attribute_ds(&ds, &conn.corner_table, fans, seams, placeholders);
        let (attributes, transforms) =
            decode_payloads(reader, &descriptors, &adss, &seeds, &group_ids)?;
        (faces, attributes, transforms)
    } else {
        // Identity path: points equal position vertices.
        let faces: Vec<[PointIdx; 3]> = (0..conn.num_faces)
            .map(|f| {
                [
                    PointIdx::from(usize::from(conn.corner_to_vertex[3 * f])),
                    PointIdx::from(usize::from(conn.corner_to_vertex[3 * f + 1])),
                    PointIdx::from(usize::from(conn.corner_to_vertex[3 * f + 2])),
                ]
            })
            .collect();
        let adss: Vec<_> = placeholders
            .into_iter()
            .map(|placeholder| {
                IdentityDS::seamless(
                    &conn.corner_table,
                    &conn.corner_to_vertex,
                    &conn.vertex_corners,
                    conn.num_vertices,
                    placeholder,
                )
            })
            .collect();
        // Every attribute rides the position connectivity: one traversal group.
        let group_ids = vec![0; num_atts];
        let (attributes, transforms) =
            decode_payloads(reader, &descriptors, &adss, &seeds, &group_ids)?;
        (faces, attributes, transforms)
    };

    Ok(DecodedAttributes {
        faces,
        attributes,
        transforms,
    })
}

/// Decodes every attribute payload over the shared connectivity `adss`.
/// Payload blocks are parsed up front in wire order; decode then runs one lazy
/// traversal per (wave, group), stepping every member attribute at each
/// emitted corner, so a shared sequence is computed exactly once and never
/// materialized. Each group walk is handed to [`GroupWalkDs::run_group`],
/// which monomorphizes the walk on the concrete attribute data structure.
fn decode_payloads<D: GroupWalkDs>(
    reader: &mut Reader<'_>,
    descriptors: &[Descriptor],
    adss: &[D],
    seeds: &[CornerIdx],
    group_ids: &[usize],
) -> Result<(Vec<Attribute>, Vec<AttributeTransform>), Err> {
    // The value count is per traversal group: the walk reaches every
    // referenced vertex exactly once. The vertex index space may carry phantom
    // ids (split merges are not compacted when seam data is present), so the
    // vertex bound alone overcounts.
    let mut group_num_values: Vec<usize> = vec![0; adss.len()];
    for rep in 0..adss.len() {
        if group_ids[rep] == rep {
            group_num_values[rep] = adss[rep].num_referenced_vertices();
        }
    }

    // Every attribute rides its own encoder here, so its payload and
    // portabilization blocks are adjacent. Both are read before the first group
    // walk starts.
    let mut parsed: Vec<Option<ParsedPayload>> = Vec::with_capacity(adss.len());
    let mut dequants: Vec<Option<AttributeTransform>> = Vec::with_capacity(adss.len());
    for (i, desc) in descriptors.iter().enumerate() {
        let num_values = group_num_values[group_ids[i]];
        parsed.push(Some(parse_payload_dispatched(reader, num_values, desc)?));
        dequants.push(Some(read_portabilization(reader, desc)?));
    }

    // Waves order the parent dependency: geometric schemes consult the decoded
    // position attribute, so they generally cannot share the walk that decodes
    // their parent. Normal prediction is the exception: its prediction never
    // reads other normals, so it rides the parent's walk with per-vertex
    // finalization deferred to the moment the one-ring positions complete
    // (see [`NormalFuser`]). When both waves do use the same traversal, the
    // first wave's walk records the corners it emits as a byproduct and the
    // second wave replays them, so a shared sequence is still computed once.
    let mut recorded_seqs: Vec<Option<Vec<CornerIdx>>> = (0..adss.len()).map(|_| None).collect();
    // The vertex-to-points adjacency per group, built on first use and shared
    // by both waves.
    let mut csrs: Vec<Option<(Vec<usize>, Vec<PointIdx>)>> =
        (0..adss.len()).map(|_| None).collect();
    let mut slots: Vec<Option<(Attribute, AttributeTransform)>> =
        (0..adss.len()).map(|_| None).collect();
    for wave in 0..2 {
        let parent_wave = wave == 1;
        for rep in 0..adss.len() {
            if group_ids[rep] != rep {
                continue;
            }
            let mut members: Vec<usize> = Vec::new();
            let mut deferred: Vec<usize> = Vec::new();
            for i in 0..adss.len() {
                if group_ids[i] != rep {
                    continue;
                }
                match parsed[i].as_ref() {
                    Some(p) if p.needs_parent() == parent_wave => members.push(i),
                    Some(_) => deferred.push(i),
                    None => {}
                }
            }
            // Normal attributes whose parent position decodes in this very
            // walk fuse into it instead of waiting for the second wave.
            let mut fused: Vec<(usize, usize)> = Vec::new();
            if !parent_wave {
                deferred.retain(|&i| {
                    let is_normal = matches!(
                        parsed[i].as_ref(),
                        Some(ParsedPayload::N2(p))
                            if p.scheme_ty == PredictionSchemeType::MeshNormalPrediction
                    );
                    let parent_stepper = descriptors[..i]
                        .iter()
                        .position(|d| d.is_prediction_parent())
                        .and_then(|j| members.iter().position(|&m| m == j));
                    match (is_normal, parent_stepper) {
                        (true, Some(s)) => {
                            fused.push((i, s));
                            false
                        }
                        _ => true,
                    }
                });
            }
            let later_wave_members = !parent_wave && !deferred.is_empty();
            if members.is_empty() && fused.is_empty() {
                continue;
            }
            let num_values = group_num_values[rep];
            let mut member_adss: Vec<&D> = Vec::with_capacity(members.len());
            let mut group_members: Vec<GroupMember<'_>> = Vec::with_capacity(members.len());
            for &i in &members {
                let parent = if parent_wave {
                    let parent = descriptors[..i].iter().zip(&slots).find_map(|(d, slot)| {
                        if d.is_prediction_parent() {
                            slot.as_ref().map(|(a, _)| a)
                        } else {
                            None
                        }
                    });
                    if parent.is_none() {
                        return Err(Err::MalformedAttribute(
                            "geometric prediction requires an already decoded position attribute",
                        ));
                    }
                    parent
                } else {
                    None
                };
                let payload = parsed[i].take().expect("each attribute joins one group");
                member_adss.push(&adss[i]);
                group_members.push(GroupMember {
                    payload,
                    dequant: dequants[i].take().expect("each attribute joins one group"),
                    desc: &descriptors[i],
                    parent,
                });
            }
            let mut fused_normals: Vec<FusedNormal<'_>> = Vec::with_capacity(fused.len());
            for &(i, parent_stepper) in &fused {
                let Some(ParsedPayload::N2(p)) = parsed[i].take() else {
                    unreachable!("fused attributes are 2-component normals");
                };
                let parent_id = AttributeId::new(descriptors[members[parent_stepper]].uid as usize);
                fused_normals.push(FusedNormal {
                    payload: p,
                    dequant: dequants[i].take().expect("each attribute joins one group"),
                    desc: &descriptors[i],
                    parent_id,
                    parent_stepper,
                });
            }
            let result = D::run_group(
                &adss[rep],
                &member_adss,
                GroupCtx {
                    members: group_members,
                    fused: fused_normals,
                    num_values,
                    seeds,
                    replay: recorded_seqs[rep].take(),
                    record_walk: later_wave_members,
                    csr: &mut csrs[rep],
                },
            )?;
            recorded_seqs[rep] = result.recorded_seq;
            for (i, r) in members.into_iter().zip(result.members) {
                slots[i] = Some(r);
            }
            for ((i, _), r) in fused.into_iter().zip(result.fused) {
                slots[i] = Some(r);
            }
        }
    }

    let mut attributes: Vec<Attribute> = Vec::with_capacity(adss.len());
    let mut transforms: Vec<AttributeTransform> = Vec::with_capacity(adss.len());
    for slot in slots {
        let (att, transform) = slot.expect("every attribute belongs to exactly one wave");
        attributes.push(att);
        transforms.push(transform);
    }
    Ok((attributes, transforms))
}

/// One member attribute's inputs to a group walk: its parsed payload, wire
/// descriptor, and the decoded parent position when the scheme predicts from
/// one.
struct GroupMember<'p> {
    payload: ParsedPayload<'p>,
    dequant: AttributeTransform,
    desc: &'p Descriptor,
    parent: Option<&'p Attribute>,
}

/// One fused normal attribute's inputs to a group walk (see [`NormalFuser`]).
struct FusedNormal<'p> {
    payload: Parsed<'p, 2>,
    dequant: AttributeTransform,
    desc: &'p Descriptor,
    parent_id: AttributeId,
    parent_stepper: usize,
}

/// The variant-independent inputs of one group walk, assembled by the
/// scheduler in [`decode_payloads`].
struct GroupCtx<'p, 'c> {
    members: Vec<GroupMember<'p>>,
    fused: Vec<FusedNormal<'p>>,
    num_values: usize,
    seeds: &'c [CornerIdx],
    /// The corners recorded by an earlier walk of this group, replayed instead
    /// of re-traversing.
    replay: Option<Vec<CornerIdx>>,
    /// Whether the walk must record its corners for a later wave.
    record_walk: bool,
    /// The group's cached vertex-to-points adjacency, built on first use.
    csr: &'c mut Option<(Vec<usize>, Vec<PointIdx>)>,
}

/// The owned results of one group walk: decoded (attribute, transform) pairs
/// for the members and the fused normals, in input order, plus the walk's
/// recorded corners when a later wave will replay it.
struct GroupResult {
    members: Vec<(Attribute, AttributeTransform)>,
    fused: Vec<(Attribute, AttributeTransform)>,
    recorded_seq: Option<Vec<CornerIdx>>,
}

/// Group-walk entry point of an attribute data structure. `run_group` hands
/// the walk to the concrete structure type, so the per-corner body is
/// monomorphized per variant: a heterogeneous collection ([`GeneralDs`])
/// dispatches once per group instead of at every hot call.
trait GroupWalkDs: Sized {
    /// See [`GenericAttributeDs::num_referenced_vertices`].
    fn num_referenced_vertices(&self) -> usize;

    /// Runs one traversal walk over `rep`'s connectivity for `ctx`'s member
    /// attributes. `member_adss` is parallel to `ctx.members`.
    fn run_group<'p>(
        rep: &'p Self,
        member_adss: &[&'p Self],
        ctx: GroupCtx<'p, '_>,
    ) -> Result<GroupResult, Err>;
}

impl<'a, CT, V> GroupWalkDs for IdentityDS<'a, CT, V>
where
    IdentityDS<'a, CT, V>: GenericAttributeDs,
{
    fn num_referenced_vertices(&self) -> usize {
        GenericAttributeDs::num_referenced_vertices(self)
    }

    fn run_group<'p>(
        rep: &'p Self,
        member_adss: &[&'p Self],
        ctx: GroupCtx<'p, '_>,
    ) -> Result<GroupResult, Err> {
        run_group_impl(rep, member_adss, ctx)
    }
}

impl GroupWalkDs for GeneralDs<'_> {
    fn num_referenced_vertices(&self) -> usize {
        match self {
            GeneralDs::Seamed(d) => GenericAttributeDs::num_referenced_vertices(d),
            GeneralDs::Finest(d) => GenericAttributeDs::num_referenced_vertices(d),
        }
    }

    fn run_group<'p>(
        rep: &'p Self,
        member_adss: &[&'p Self],
        ctx: GroupCtx<'p, '_>,
    ) -> Result<GroupResult, Err> {
        // A traversal group is keyed by seam-set equality, and equal seam sets
        // build identical structures, so every member shares the rep's variant.
        match rep {
            GeneralDs::Seamed(r) => {
                let members: Vec<_> = member_adss
                    .iter()
                    .map(|&m| match m {
                        GeneralDs::Seamed(d) => d,
                        GeneralDs::Finest(_) => {
                            unreachable!("group member variant differs from its rep")
                        }
                    })
                    .collect();
                run_group_impl(r, &members, ctx)
            }
            GeneralDs::Finest(r) => {
                let members: Vec<_> = member_adss
                    .iter()
                    .map(|&m| match m {
                        GeneralDs::Finest(d) => d,
                        GeneralDs::Seamed(_) => {
                            unreachable!("group member variant differs from its rep")
                        }
                    })
                    .collect();
                run_group_impl(r, &members, ctx)
            }
        }
    }
}

/// Builds the group's steppers and fusers and runs one traversal walk,
/// monomorphized on the concrete attribute data structure.
fn run_group_impl<'p, D: GenericAttributeDs>(
    rep: &'p D,
    member_adss: &[&'p D],
    ctx: GroupCtx<'p, '_>,
) -> Result<GroupResult, Err> {
    let GroupCtx {
        members,
        fused,
        num_values,
        seeds,
        replay,
        record_walk,
        csr,
    } = ctx;
    debug_assert_eq!(members.len(), member_adss.len());
    let mut steppers: Vec<AnyStepper<'_, D>> = Vec::with_capacity(members.len());
    for (m, &ads) in members.into_iter().zip(member_adss) {
        steppers.push(build_stepper(
            m.payload, m.dequant, m.desc, ads, m.parent, num_values,
        )?);
    }
    let mut fusers: Vec<NormalFuser> = Vec::with_capacity(fused.len());
    for f in fused {
        fusers.push(NormalFuser::new(
            f.payload,
            f.dequant,
            f.desc,
            f.parent_id,
            rep,
            f.parent_stepper,
            num_values,
        ));
    }
    let csr_ref = if rep.point_equals_vertex() {
        None
    } else {
        if csr.is_none() {
            *csr = Some(vertex_points_csr(rep));
        }
        csr.as_ref()
    };
    let recorded_seq = match replay {
        Some(seq) => {
            decode_group(
                rep,
                seq.iter().copied(),
                &mut steppers,
                &mut fusers,
                num_values,
                None,
                csr_ref,
            )?;
            None
        }
        None => {
            let mut record_seq = record_walk.then(|| Vec::with_capacity(num_values));
            decode_group(
                rep,
                Traverser::new(rep, seeds.to_vec()),
                &mut steppers,
                &mut fusers,
                num_values,
                record_seq.as_mut(),
                csr_ref,
            )?;
            record_seq
        }
    };
    Ok(GroupResult {
        members: steppers.into_iter().map(AnyStepper::finish).collect(),
        fused: fusers.into_iter().map(NormalFuser::finish).collect(),
        recorded_seq,
    })
}

/// One attribute's correction stream: DirectCoded corrections stay behind a
/// live decoder popped in walk order (the walk consumes ranks sequentially,
/// which is exactly the decoder's output order); raw and LengthCoded ones are
/// rank-indexed vectors read before the walk.
enum Corrections<'a, const N: usize>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    Eager(Vec<NdVector<N, i32>>),
    Lazy(RansSymbolDecoder<'a>),
}

impl<const N: usize> Corrections<'_, N>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    /// The correction of rank `k`. Lazy streams require the calls to arrive
    /// with consecutive `k`, which the group walk guarantees.
    ///
    /// # Safety
    /// `k` must be less than the stream's value count.
    #[inline]
    unsafe fn next_unchecked(&mut self, k: usize) -> NdVector<N, i32> {
        match self {
            Corrections::Eager(v) => *v.get_unchecked(k),
            Corrections::Lazy(d) => {
                let mut v = NdVector::<N, i32>::zero();
                for i in 0..N {
                    *v.get_mut(i) = d.decode() as u32 as i32;
                }
                v
            }
        }
    }

    /// Drains the stream into a rank-indexed vector, for consumers that read
    /// corrections out of walk order.
    fn materialize(self, num_values: usize) -> Vec<NdVector<N, i32>> {
        match self {
            Corrections::Eager(v) => v,
            Corrections::Lazy(mut d) => (0..num_values)
                .map(|_| {
                    let mut v = NdVector::<N, i32>::zero();
                    for i in 0..N {
                        *v.get_mut(i) = d.decode() as u32 as i32;
                    }
                    v
                })
                .collect(),
        }
    }
}

/// One attribute's parsed payload block: prediction/transform ids, the
/// correction stream, and the scheme and transform metadata (in the
/// scheme-dependent order the encoder writes them). The portabilization
/// parameters live in their own block, read by [`read_portabilization`].
struct Parsed<'a, const N: usize>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    scheme_ty: PredictionSchemeType,
    corrections: Corrections<'a, N>,
    flips: Vec<bool>,
    orientations: Vec<bool>,
    transform: InverseTransform,
}

/// [`Parsed`] behind the component-count dispatch.
enum ParsedPayload<'a> {
    N1(Parsed<'a, 1>),
    N2(Parsed<'a, 2>),
    N3(Parsed<'a, 3>),
    N4(Parsed<'a, 4>),
}

impl ParsedPayload<'_> {
    fn scheme_ty(&self) -> &PredictionSchemeType {
        match self {
            ParsedPayload::N1(p) => &p.scheme_ty,
            ParsedPayload::N2(p) => &p.scheme_ty,
            ParsedPayload::N3(p) => &p.scheme_ty,
            ParsedPayload::N4(p) => &p.scheme_ty,
        }
    }

    /// Whether the scheme predicts from a decoded position attribute, which
    /// places the attribute in the second decode wave.
    fn needs_parent(&self) -> bool {
        matches!(
            self.scheme_ty(),
            PredictionSchemeType::MeshNormalPrediction
                | PredictionSchemeType::MeshPredictionForTextureCoordinates
        )
    }
}

/// [`parse_payload`] behind the portable component-count dispatch.
fn parse_payload_dispatched<'a>(
    reader: &mut Reader<'a>,
    num_values: usize,
    desc: &Descriptor,
) -> Result<ParsedPayload<'a>, Err> {
    Ok(match desc.portable_num_components() {
        1 => ParsedPayload::N1(parse_payload::<1>(reader, num_values, desc)?),
        2 => ParsedPayload::N2(parse_payload::<2>(reader, num_values, desc)?),
        3 => ParsedPayload::N3(parse_payload::<3>(reader, num_values, desc)?),
        4 => ParsedPayload::N4(parse_payload::<4>(reader, num_values, desc)?),
        _ => return Err(Err::MalformedAttribute("unsupported number of components")),
    })
}

/// Parses one attribute's payload block off the wire.
fn parse_payload<'a, const N: usize>(
    reader: &mut Reader<'a>,
    num_values: usize,
    desc: &Descriptor,
) -> Result<Parsed<'a, N>, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
{
    // The generic encoder writes no header at all: the payload is the values
    // themselves, in their declared component type. Modelled as an unpredicted,
    // untransformed stream so the reversal walk is shared with every other
    // encoder.
    if desc.port_type == PORT_GENERIC {
        let values = read_raw_values::<N>(reader, num_values, desc.component_type)?;
        return Ok(Parsed {
            scheme_ty: PredictionSchemeType::NoPrediction,
            corrections: Corrections::Eager(values),
            flips: Vec::new(),
            orientations: Vec::new(),
            transform: InverseTransform::None,
        });
    }

    let scheme_ty = prediction::read_scheme_id(reader)?;
    let transform_id = reader.read_u8()?;

    // The correction stream: DirectCoded symbols stay behind a live decoder
    // drained during the walk (the payload is length-prefixed, so parsing
    // continues past it), raw values are read eagerly. LengthCoded symbols are
    // drained here instead, which keeps the walk's per-symbol pop monomorphic
    // over the DirectCoded decoder.
    let rans_flag = reader.read_u8()?;
    let corrections: Corrections<'a, N> = if rans_flag != 0 {
        match start_symbol_decoder(reader, num_values * N, N)? {
            AnySymbolDecoder::Direct(decoder) => Corrections::Lazy(decoder),
            AnySymbolDecoder::Tagged(mut decoder) => {
                let mut out = Vec::with_capacity(num_values);
                for _ in 0..num_values {
                    let mut v = NdVector::<N, i32>::zero();
                    for i in 0..N {
                        *v.get_mut(i) = decoder.decode() as u32 as i32;
                    }
                    out.push(v);
                }
                Corrections::Eager(out)
            }
        }
    } else {
        let mut out = Vec::with_capacity(num_values);
        for _ in 0..num_values {
            out.push(NdVector::<N, i32>::read_from(reader)?);
        }
        Corrections::Eager(out)
    };

    // Scheme and transform metadata, in the order the encoder writes them.
    let mut flips = Vec::new();
    let mut orientations = Vec::new();
    let transform = match scheme_ty {
        PredictionSchemeType::MeshNormalPrediction => {
            let t = InverseTransform::read_from(reader, transform_id)?;
            flips = prediction::decode_flip_metadata(reader, num_values)?;
            t
        }
        PredictionSchemeType::MeshPredictionForTextureCoordinates => {
            orientations = prediction::decode_orientation_metadata(reader)?;
            InverseTransform::read_from(reader, transform_id)?
        }
        _ => InverseTransform::read_from(reader, transform_id)?,
    };

    Ok(Parsed {
        scheme_ty,
        corrections,
        flips,
        orientations,
        transform,
    })
}

/// Reads a generic attribute's values: `num_values` entries of `N` components,
/// each the little-endian encoding of one `component_type`, widened into the
/// i32 the portable representation carries. Floats travel as their bit pattern
/// and unsigned types as their two's complement; both are reversed by
/// [`AttributeTransform::Raw`].
fn read_raw_values<const N: usize>(
    reader: &mut Reader<'_>,
    num_values: usize,
    component_type: ComponentDataType,
) -> Result<Vec<NdVector<N, i32>>, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    // 64-bit components do not fit the i32 portable representation.
    if component_type.size() > 4 {
        return Err(Err::Unimplemented);
    }
    let mut out = Vec::with_capacity(num_values);
    for _ in 0..num_values {
        let mut v = NdVector::<N, i32>::zero();
        for i in 0..N {
            *v.get_mut(i) = match component_type {
                ComponentDataType::I8 => reader.read_u8()? as i8 as i32,
                ComponentDataType::U8 => reader.read_u8()? as i32,
                ComponentDataType::I16 => reader.read_u16()? as i16 as i32,
                ComponentDataType::U16 => reader.read_u16()? as i32,
                ComponentDataType::I32 | ComponentDataType::U32 | ComponentDataType::F32 => {
                    reader.read_u32()? as i32
                }
                _ => return Err(Err::MalformedAttribute("invalid component type")),
            };
        }
        out.push(v);
    }
    Ok(out)
}

/// One attribute's state through a group walk: the partially filled attribute,
/// its predictor, and the parsed correction/transform data. Each emitted corner
/// advances every stepper of the group by one value.
struct Stepper<'p, const N: usize, D: GenericAttributeDs>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    att: Attribute,
    predictor: Predictor<'p, N, D>,
    transform: InverseTransform,
    corrections: Corrections<'p, N>,
    zigzagged: bool,
    dequant: AttributeTransform,
}

impl<'p, const N: usize, D: GenericAttributeDs> Stepper<'p, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
{
    fn new(
        parsed: Parsed<'p, N>,
        dequant: AttributeTransform,
        desc: &Descriptor,
        ads: &'p D,
        parent: Option<&'p Attribute>,
        num_values: usize,
    ) -> Result<Self, Err> {
        let parents_ids = parent.map(|p| vec![p.get_id()]).unwrap_or_default();
        let mut att = Attribute::from_without_removing_duplicates::<NdVector<N, i32>, N>(
            AttributeId::new(desc.uid as usize),
            vec![NdVector::<N, i32>::zero(); num_values],
            desc.att_type,
            desc.domain,
            parents_ids,
        );
        att.set_point_to_att_val_map(Some(VecPointIdx::from(vec![
            AttributeValueIdx::from(0);
            ads.num_points()
        ])));
        let parent_refs: Vec<&Attribute> = parent.into_iter().collect();
        let predictor = Predictor::<N, D>::new(
            &parsed.scheme_ty,
            &parent_refs,
            ads,
            parsed.flips,
            parsed.orientations,
            parsed.transform.oct_center(),
        )?;
        let zigzagged = parsed.transform.corrections_are_zigzagged();
        Ok(Self {
            att,
            predictor,
            transform: parsed.transform,
            corrections: parsed.corrections,
            zigzagged,
            dequant,
        })
    }

    /// Decodes this attribute's value of rank `k` at the emitted corner `c`,
    /// after mapping the corner's fan `points` to `k`.
    #[inline]
    fn step(&mut self, c: CornerIdx, k: usize, points: &[PointIdx], record: &[VertexIdx]) {
        // SAFETY: the walk emits at most one corner per distinct vertex, so
        // k < num_values, the length of both the value buffer and any eager
        // correction vector; `points` holds point ids of the walked structure,
        // all below the map length the stepper was constructed with.
        for &p in points {
            unsafe {
                self.att
                    .set_point_att_val_unchecked(p, AttributeValueIdx::from(k));
            }
        }
        // Predictions only ever read values at already visited vertices, so
        // the partially filled attribute is safe to consult.
        let pred = self.predictor.predict(c, record, &self.att);
        let mut corr = unsafe { self.corrections.next_unchecked(k) };
        if self.zigzagged {
            for i in 0..N {
                *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
            }
        }
        unsafe {
            *self
                .att
                .unique_vals_as_slice_unchecked_mut::<NdVector<N, i32>>()
                .get_unchecked_mut(k) = self.transform.compute_original(pred, corr);
        }
    }
}

/// [`Stepper`] behind the component-count dispatch, so one group walk drives
/// attributes of different component counts.
enum AnyStepper<'p, D: GenericAttributeDs> {
    N1(Stepper<'p, 1, D>),
    N2(Stepper<'p, 2, D>),
    N3(Stepper<'p, 3, D>),
    N4(Stepper<'p, 4, D>),
}

impl<'p, D: GenericAttributeDs> AnyStepper<'p, D> {
    #[inline]
    fn step(&mut self, c: CornerIdx, k: usize, points: &[PointIdx], record: &[VertexIdx]) {
        match self {
            AnyStepper::N1(s) => s.step(c, k, points, record),
            AnyStepper::N2(s) => s.step(c, k, points, record),
            AnyStepper::N3(s) => s.step(c, k, points, record),
            AnyStepper::N4(s) => s.step(c, k, points, record),
        }
    }

    fn finish(self) -> (Attribute, AttributeTransform) {
        match self {
            AnyStepper::N1(s) => (s.att, s.dequant),
            AnyStepper::N2(s) => (s.att, s.dequant),
            AnyStepper::N3(s) => (s.att, s.dequant),
            AnyStepper::N4(s) => (s.att, s.dequant),
        }
    }

    fn att(&self) -> &Attribute {
        match self {
            AnyStepper::N1(s) => &s.att,
            AnyStepper::N2(s) => &s.att,
            AnyStepper::N3(s) => &s.att,
            AnyStepper::N4(s) => &s.att,
        }
    }
}

fn build_stepper<'p, D: GenericAttributeDs>(
    payload: ParsedPayload<'p>,
    dequant: AttributeTransform,
    desc: &Descriptor,
    ads: &'p D,
    parent: Option<&'p Attribute>,
    num_values: usize,
) -> Result<AnyStepper<'p, D>, Err> {
    Ok(match payload {
        ParsedPayload::N1(p) => {
            AnyStepper::N1(Stepper::new(p, dequant, desc, ads, parent, num_values)?)
        }
        ParsedPayload::N2(p) => {
            AnyStepper::N2(Stepper::new(p, dequant, desc, ads, parent, num_values)?)
        }
        ParsedPayload::N3(p) => {
            AnyStepper::N3(Stepper::new(p, dequant, desc, ads, parent, num_values)?)
        }
        ParsedPayload::N4(p) => {
            AnyStepper::N4(Stepper::new(p, dequant, desc, ads, parent, num_values)?)
        }
    })
}

/// A geometric-normal attribute fused into its parent position's walk. The
/// prediction at a vertex is the sum of its one-ring face normals, which never
/// reads other normal values, so decode rides the position walk instead of a
/// second one: during the walk only the point map and per-vertex ranks are
/// recorded, and once the walk ends (every position decoded by construction)
/// one sequential face pass computes each cross product exactly once and each
/// vertex finalizes from its completed sum. The sums equal the eager
/// full-mesh pass bit for bit (exact i64 sums, order independent).
struct NormalFuser {
    att: Attribute,
    corrections: Vec<NdVector<2, i32>>,
    flips: Vec<bool>,
    zigzagged: bool,
    transform: InverseTransform,
    dequant: AttributeTransform,
    /// Index of the parent position attribute within the group's steppers.
    parent_stepper: usize,
    /// Traversal rank per vertex, recorded at emit; `usize::MAX` for vertices
    /// the walk never reaches (phantom ids).
    rank_of: Vec<usize>,
}

impl NormalFuser {
    fn new<D: GenericAttributeDs>(
        parsed: Parsed<'_, 2>,
        dequant: AttributeTransform,
        desc: &Descriptor,
        parent_id: AttributeId,
        ads: &D,
        parent_stepper: usize,
        num_values: usize,
    ) -> Self {
        let mut att = Attribute::from_without_removing_duplicates::<NdVector<2, i32>, 2>(
            AttributeId::new(desc.uid as usize),
            vec![NdVector::<2, i32>::zero(); num_values],
            desc.att_type,
            desc.domain,
            vec![parent_id],
        );
        att.set_point_to_att_val_map(Some(VecPointIdx::from(vec![
            AttributeValueIdx::from(0);
            ads.num_points()
        ])));
        let zigzagged = parsed.transform.corrections_are_zigzagged();
        Self {
            att,
            // The end-of-walk finalization reads corrections in vertex order,
            // not rank order, so the stream cannot stay lazy here.
            corrections: parsed.corrections.materialize(num_values),
            flips: parsed.flips,
            zigzagged,
            transform: parsed.transform,
            dequant,
            parent_stepper,
            rank_of: vec![usize::MAX; ads.vertex_index_bound()],
        }
    }

    /// Records the emit of vertex `v` at rank `k`: maps the vertex's `points`
    /// to `k` and remembers the rank for the end-of-walk finalization.
    #[inline]
    fn on_emit(&mut self, v: VertexIdx, k: usize, points: &[PointIdx]) {
        for &p in points {
            self.att.set_point_att_val(p, AttributeValueIdx::from(k));
        }
        self.rank_of[usize::from(v)] = k;
    }

    /// Decodes every normal value after the walk: accumulates the face-normal
    /// sums in one sequential face pass over the completed positions, then
    /// applies each vertex's flip and correction at its recorded rank.
    fn finish_walk<D: GenericAttributeDs>(&mut self, ads: &D, pos: &Attribute) {
        let center = self.transform.oct_center();
        let sums = accumulate_face_normal_sums(ads, pos, self.rank_of.len());
        for (v, &k) in self.rank_of.iter().enumerate() {
            if k == usize::MAX {
                continue;
            }
            let mut pred_3d = sum_to_canonical_normal(sums[v], center);
            if self.flips.get(k).copied().unwrap_or(false) {
                pred_3d *= -1;
            }
            let pred = canonical_normal_to_oct::<2>(pred_3d, center);
            let mut corr = self.corrections[k];
            if self.zigzagged {
                for i in 0..2 {
                    *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
                }
            }
            self.att.unique_vals_as_slice_mut::<NdVector<2, i32>>()[k] =
                self.transform.compute_original(pred, corr);
        }
    }

    fn finish(self) -> (Attribute, AttributeTransform) {
        (self.att, self.dequant)
    }
}

/// Builds the vertex-to-points adjacency of `ads` in CSR form:
/// `points[offsets[v]..offsets[v + 1]]` are the points of vertex `v`.
fn vertex_points_csr<D: GenericAttributeDs>(ads: &D) -> (Vec<usize>, Vec<PointIdx>) {
    let num_vertices = ads.vertex_index_bound();
    let num_points = ads.num_points();
    let mut offsets = vec![0usize; num_vertices + 1];
    for p in 0..num_points {
        offsets[usize::from(ads.point_to_vertex(PointIdx::from(p))) + 1] += 1;
    }
    for v in 0..num_vertices {
        offsets[v + 1] += offsets[v];
    }
    let mut cursor = offsets.clone();
    let mut points = vec![PointIdx::from(0); num_points];
    for p in 0..num_points {
        let v = usize::from(ads.point_to_vertex(PointIdx::from(p)));
        points[cursor[v]] = PointIdx::from(p);
        cursor[v] += 1;
    }
    (offsets, points)
}

/// Runs one traversal walk for a group of attributes sharing the same
/// connectivity, stepping every attribute at each emitted corner. Point-map
/// entries are filled as vertices are emitted: predictions only read neighbors
/// of already emitted vertices, whose fan corners are already mapped. When
/// `record_seq` is given, the emitted corners are recorded so a later wave can
/// replay the walk without recomputing it. `csr` is the vertex-to-points
/// adjacency from [`vertex_points_csr`]; `None` means points equal vertices.
fn decode_group<D: GenericAttributeDs>(
    ads: &D,
    walk: impl Iterator<Item = CornerIdx>,
    steppers: &mut [AnyStepper<'_, D>],
    fusers: &mut [NormalFuser],
    num_values: usize,
    mut record_seq: Option<&mut Vec<CornerIdx>>,
    csr: Option<&(Vec<usize>, Vec<PointIdx>)>,
) -> Result<(), Err> {
    let mut record: Vec<VertexIdx> = Vec::with_capacity(num_values);
    #[cfg(debug_assertions)]
    let mut point_mapped = vec![false; ads.num_points()];
    let mut k = 0;
    for c in walk {
        if let Some(seq) = record_seq.as_mut() {
            seq.push(c);
        }
        let v = ads.vertex_idx(c);
        let own_point;
        let points: &[PointIdx] = match csr {
            None => {
                own_point = [PointIdx::from(usize::from(v))];
                &own_point
            }
            // SAFETY: the CSR is built from the same `ads` as the walk, so
            // v < vertex_index_bound = offsets.len() - 1, and the offsets are
            // monotone prefix sums bounded by vertex_points.len().
            Some((offsets, vertex_points)) => unsafe {
                let start = *offsets.get_unchecked(usize::from(v));
                let end = *offsets.get_unchecked(usize::from(v) + 1);
                vertex_points.get_unchecked(start..end)
            },
        };
        #[cfg(debug_assertions)]
        for &p in points {
            point_mapped[usize::from(p)] = true;
        }
        for s in steppers.iter_mut() {
            s.step(c, k, points, &record);
        }
        for nf in fusers.iter_mut() {
            nf.on_emit(v, k, points);
        }
        record.push(v);
        k += 1;
    }
    if k != num_values {
        return Err(Err::MalformedAttribute(
            "traversal did not reach every attribute vertex",
        ));
    }
    // Every position is decoded once the walk completes, so the fused normals
    // can run their sequential face pass and finalize.
    for nf in fusers.iter_mut() {
        let pos = steppers[nf.parent_stepper].att();
        nf.finish_walk(ads, pos);
    }
    // Phantom points (never referenced by a corner) legitimately stay
    // unmapped; every referenced point must have been covered by some fan.
    #[cfg(debug_assertions)]
    for c in 0..ads.num_corners() {
        let p = usize::from(ads.point_idx(CornerIdx::from(c)));
        debug_assert!(point_mapped[p], "fan fill left a referenced point unmapped");
    }
    Ok(())
}

/// Parses one attribute's portabilization metadata into the dequantization
/// parameters surfaced through [`AttributeTransform`]. This block is written
/// separately from the payload: an encoder carrying several attributes emits
/// every payload before the first of these.
fn read_portabilization(
    reader: &mut Reader<'_>,
    desc: &Descriptor,
) -> Result<AttributeTransform, Err> {
    match desc.port_type {
        PORT_GENERIC => Ok(AttributeTransform::Raw {
            component_type: desc.component_type,
        }),
        PORT_TO_BITS => Ok(AttributeTransform::None),
        PORT_QUANTIZATION_COORDINATE_WISE => {
            let min = (0..desc.portable_num_components())
                .map(|_| f32::read_from(reader))
                .collect::<Result<Vec<_>, _>>()?;
            let delta_max = f32::read_from(reader)?;
            let bits = reader.read_u8()?;
            Ok(AttributeTransform::Quantized {
                min,
                delta_max,
                bits,
            })
        }
        PORT_OCTAHEDRAL => Ok(AttributeTransform::Octahedral {
            bits: reader.read_u8()?,
        }),
        _ => Err(Err::MalformedAttribute("unknown portabilization type")),
    }
}
