//! Attribute decoding: framing, group walks, and point assembly.

mod inverse_transform;
mod lazy;
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
use draco_oxide_core::mesh::ds::{CornerTable, GenericAttributeDs, IdentityDS};
use draco_oxide_core::types::{
    AttributeValueIdx, CornerIdx, NdVector, PointIdx, VecPointIdx, Vector, VertexIdx,
};
use draco_oxide_core::utils::bit_coder::leb128_read;

use inverse_transform::InverseTransform;
use prediction::Predictor;

/// The identity structure over the position connectivity, shared by every
/// attribute without interior seams.
type PlainDs<'a> = IdentityDS<'a, &'a CornerTable, VertexIdx>;

/// The portabilization type ids on the wire.
const PORT_GENERIC: u8 = 0;
const PORT_TO_BITS: u8 = 1;
const PORT_QUANTIZATION_COORDINATE_WISE: u8 = 2;
const PORT_OCTAHEDRAL: u8 = 3;

/// The wire id of an attribute riding the position connectivity.
const NO_ATTRIBUTE_DATA: u8 = 0xFF;

/// The decoded attribute section.
pub(crate) struct DecodedAttributes {
    pub faces: Vec<[PointIdx; 3]>,
    pub attributes: Vec<Attribute>,
    pub transforms: Vec<AttributeTransform>,
}

/// One attribute's wire descriptor.
struct Descriptor {
    att_data_id: u8,
    att_type: AttributeType,
    component_type: ComponentDataType,
    num_components: usize,
    uid: u32,
    port_type: u8,
    domain: AttributeDomain,
}

impl Descriptor {
    /// Whether this attribute can be a prediction parent.
    fn is_prediction_parent(&self) -> bool {
        self.att_type == AttributeType::Position && self.port_type != PORT_GENERIC
    }

    /// Components of the portable representation; octahedral travels as 2.
    fn portable_num_components(&self) -> usize {
        if self.port_type == PORT_OCTAHEDRAL {
            2
        } else {
            self.num_components
        }
    }
}

/// Decodes the whole attribute section.
pub(crate) fn decode_attributes(
    reader: &mut Reader<'_>,
    conn: &Connectivity,
) -> Result<DecodedAttributes, Err> {
    match conn {
        Connectivity::Edgebreaker(conn) => decode_traversed_attributes(reader, conn),
        Connectivity::Sequential(conn) => sequential::decode_attributes(reader, conn),
    }
}

/// Decodes the attribute section of an edgebreaker stream.
fn decode_traversed_attributes(
    reader: &mut Reader<'_>,
    conn: &EdgebreakerConnectivity,
) -> Result<DecodedAttributes, Err> {
    let num_atts = reader.read_u8()? as usize;

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

    let seam_bits: &[u8] = &conn.seam_bits;
    let mut masks: Vec<u8> = Vec::with_capacity(num_atts);
    let mut has_interior: Vec<bool> = Vec::with_capacity(num_atts);
    let mut stats: Vec<Option<&crate::connectivity::edgebreaker::SeamStats>> =
        Vec::with_capacity(num_atts);
    for desc in &descriptors {
        if desc.domain == AttributeDomain::Corner && desc.att_data_id != NO_ATTRIBUTE_DATA {
            let d = desc.att_data_id as usize;
            let st = conn.seam_stats.get(d).ok_or(Err::MalformedAttribute(
                "attribute names a seam stream the connectivity did not encode",
            ))?;
            masks.push(1 << d);
            has_interior.push(st.has_interior);
            stats.push(Some(st));
        } else {
            masks.push(0);
            has_interior.push(false);
            stats.push(None);
        }
    }

    let seeds = sequence::traversal_seeds(conn.num_faces);

    let columns_equal =
        |mi: u8, mj: u8| mi == mj || seam_bits.iter().all(|&b| (b & mi != 0) == (b & mj != 0));
    let mut group_ids: Vec<usize> = Vec::with_capacity(num_atts);
    let mut first_plain: Option<usize> = None;
    for i in 0..num_atts {
        let gid = if !has_interior[i] {
            *first_plain.get_or_insert(i)
        } else {
            (0..i)
                .find(|&j| has_interior[j] && columns_equal(masks[j], masks[i]))
                .map(|j| group_ids[j])
                .unwrap_or(i)
        };
        group_ids.push(gid);
    }

    let shared = lazy::Shared {
        pos_ct: &conn.corner_table,
        c2v: &conn.corner_to_vertex,
    };

    let no_att = Attribute::new_empty(
        AttributeId::new(0),
        AttributeType::Invalid,
        AttributeDomain::Corner,
        ComponentDataType::I32,
        1,
    );
    let plain_ds = PlainDs::seamless(
        &conn.corner_table,
        &conn.corner_to_vertex,
        &conn.vertex_corners,
        conn.num_vertices,
        no_att,
    );

    let referenced = plain_ds.num_referenced_vertices();
    let mut group_num_values: Vec<usize> = vec![0; num_atts];
    for rep in 0..num_atts {
        if group_ids[rep] != rep {
            continue;
        }
        group_num_values[rep] = if has_interior[rep] {
            let st = stats[rep].expect("a lazy attribute always has a seam stream");
            st.starts + referenced - st.fans_with_starts
        } else {
            referenced
        };
    }

    let union_mask: u8 = (0..num_atts)
        .filter(|&r| group_ids[r] == r && has_interior[r])
        .map(|r| masks[r])
        .fold(0, |acc, m| acc | m);

    let ctx = SchedulerCtx {
        shared,
        plain_ds: &plain_ds,
        seam_bits,
        masks: &masks,
        has_interior: &has_interior,
        group_ids: &group_ids,
        group_num_values: &group_num_values,
        seeds: &seeds,
        union_mask,
    };
    let decoded = decode_payloads(reader, &descriptors, &ctx)?;
    let DecodedPayloads {
        mut attributes,
        transforms,
        assembled,
        rank_maps,
    } = decoded;

    let faces = if union_mask != 0 {
        let points = assembled.expect("a lazy group's walk assembled the points");
        for (i, att) in attributes.iter_mut().enumerate() {
            let map: Vec<AttributeValueIdx> = if has_interior[i] {
                let rank = rank_maps[group_ids[i]]
                    .as_deref()
                    .expect("every lazy group was walked");
                points
                    .rep_corners
                    .iter()
                    .map(|&c| AttributeValueIdx::from(rank[usize::from(c)] as usize))
                    .collect()
            } else {
                let old_map = att
                    .point_map_as_slice()
                    .expect("plain attributes decode over a vertex-indexed map");
                points
                    .rep_corners
                    .iter()
                    .map(|&c| old_map[usize::from(conn.corner_to_vertex[usize::from(c)])])
                    .collect()
            };
            att.set_point_to_att_val_map(Some(VecPointIdx::from(map)));
        }
        points
            .corner_to_point
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect()
    } else {
        (0..conn.num_faces)
            .map(|f| {
                [
                    PointIdx::from(usize::from(conn.corner_to_vertex[3 * f])),
                    PointIdx::from(usize::from(conn.corner_to_vertex[3 * f + 1])),
                    PointIdx::from(usize::from(conn.corner_to_vertex[3 * f + 2])),
                ]
            })
            .collect()
    };

    Ok(DecodedAttributes {
        faces,
        attributes,
        transforms,
    })
}

/// The connectivity context the payload scheduler dispatches groups over.
struct SchedulerCtx<'a> {
    shared: lazy::Shared<'a>,
    plain_ds: &'a PlainDs<'a>,
    seam_bits: &'a [u8],
    masks: &'a [u8],
    has_interior: &'a [bool],
    group_ids: &'a [usize],
    group_num_values: &'a [usize],
    seeds: &'a [CornerIdx],
    union_mask: u8,
}

/// The scheduler's outputs.
struct DecodedPayloads {
    attributes: Vec<Attribute>,
    transforms: Vec<AttributeTransform>,
    assembled: Option<lazy::AssembledPoints>,
    rank_maps: Vec<Option<Vec<u32>>>,
}

/// Parses every payload, then runs one traversal per (wave, group).
fn decode_payloads(
    reader: &mut Reader<'_>,
    descriptors: &[Descriptor],
    ctx: &SchedulerCtx<'_>,
) -> Result<DecodedPayloads, Err> {
    let num_atts = descriptors.len();
    let group_ids = ctx.group_ids;

    let mut parsed: Vec<Option<ParsedPayload>> = Vec::with_capacity(num_atts);
    let mut dequants: Vec<Option<AttributeTransform>> = Vec::with_capacity(num_atts);
    for (i, desc) in descriptors.iter().enumerate() {
        let num_values = ctx.group_num_values[group_ids[i]];
        parsed.push(Some(parse_payload_dispatched(reader, num_values, desc)?));
        dequants.push(Some(read_portabilization(reader, desc)?));
    }

    // Any lazy real walk can carry the point assembly; the last rep is a
    // deterministic choice.
    let assembly_rep: Option<usize> = (0..num_atts)
        .rev()
        .find(|&r| group_ids[r] == r && ctx.has_interior[r]);

    let mut recorded_seqs: Vec<Option<Vec<CornerIdx>>> = (0..num_atts).map(|_| None).collect();
    let mut rank_maps: Vec<Option<Vec<u32>>> = (0..num_atts).map(|_| None).collect();
    let mut assembled: Option<lazy::AssembledPoints> = None;
    let mut slots: Vec<Option<(Attribute, AttributeTransform)>> =
        (0..num_atts).map(|_| None).collect();
    for wave in 0..2 {
        let parent_wave = wave == 1;
        for rep in 0..num_atts {
            if group_ids[rep] != rep {
                continue;
            }
            let mut members: Vec<usize> = Vec::new();
            let mut deferred: Vec<usize> = Vec::new();
            for i in 0..num_atts {
                if group_ids[i] != rep {
                    continue;
                }
                match parsed[i].as_ref() {
                    Some(p) if p.needs_parent() == parent_wave => members.push(i),
                    Some(_) => deferred.push(i),
                    None => {}
                }
            }
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
            let num_values = ctx.group_num_values[rep];

            let mut parents: Vec<Option<&Attribute>> = Vec::with_capacity(members.len());
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
                parents.push(parent);
            }

            if ctx.has_interior[rep] {
                debug_assert!(fused.is_empty());
                let mut lazy_members: Vec<lazy::LazyMember<'_>> = Vec::with_capacity(members.len());
                for (&i, parent) in members.iter().zip(&parents) {
                    lazy_members.push(lazy::LazyMember {
                        payload: parsed[i].take().expect("each attribute joins one group"),
                        dequant: dequants[i].take().expect("each attribute joins one group"),
                        desc: &descriptors[i],
                        parent: *parent,
                    });
                }
                let rank_slot = &mut rank_maps[rep];
                let assemble =
                    (Some(rep) == assembly_rep && rank_slot.is_none()).then_some(ctx.union_mask);
                let outcome = lazy::run_group(
                    ctx.shared,
                    ctx.seam_bits,
                    ctx.masks[rep],
                    lazy_members,
                    lazy::LazyIo {
                        num_values,
                        seeds: ctx.seeds,
                        replay: recorded_seqs[rep].take(),
                        record_walk: later_wave_members,
                        rank: rank_slot,
                        assemble,
                    },
                )?;
                recorded_seqs[rep] = outcome.recorded;
                if outcome.points.is_some() {
                    assembled = outcome.points;
                }
                for (i, r) in members.into_iter().zip(outcome.members) {
                    slots[i] = Some(r);
                }
            } else {
                let mut group_members: Vec<GroupMember<'_>> = Vec::with_capacity(members.len());
                for (&i, parent) in members.iter().zip(&parents) {
                    group_members.push(GroupMember {
                        payload: parsed[i].take().expect("each attribute joins one group"),
                        dequant: dequants[i].take().expect("each attribute joins one group"),
                        desc: &descriptors[i],
                        parent: *parent,
                    });
                }
                let mut fused_normals: Vec<FusedNormal<'_>> = Vec::with_capacity(fused.len());
                for &(i, parent_stepper) in &fused {
                    let Some(ParsedPayload::N2(p)) = parsed[i].take() else {
                        unreachable!("fused attributes are 2-component normals");
                    };
                    let parent_id =
                        AttributeId::new(descriptors[members[parent_stepper]].uid as usize);
                    fused_normals.push(FusedNormal {
                        payload: p,
                        dequant: dequants[i].take().expect("each attribute joins one group"),
                        desc: &descriptors[i],
                        parent_id,
                        parent_stepper,
                    });
                }
                let result = run_plain_group(
                    ctx.plain_ds,
                    GroupCtx {
                        members: group_members,
                        fused: fused_normals,
                        num_values,
                        seeds: ctx.seeds,
                        replay: recorded_seqs[rep].take(),
                        record_walk: later_wave_members,
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
    }

    let mut attributes: Vec<Attribute> = Vec::with_capacity(num_atts);
    let mut transforms: Vec<AttributeTransform> = Vec::with_capacity(num_atts);
    for slot in slots {
        let (att, transform) = slot.expect("every attribute belongs to exactly one wave");
        attributes.push(att);
        transforms.push(transform);
    }
    Ok(DecodedPayloads {
        attributes,
        transforms,
        assembled,
        rank_maps,
    })
}

/// One member attribute's inputs to a plain group walk.
struct GroupMember<'p> {
    payload: ParsedPayload<'p>,
    dequant: AttributeTransform,
    desc: &'p Descriptor,
    parent: Option<&'p Attribute>,
}

/// A normal fused into its parent position's walk.
struct FusedNormal<'p> {
    payload: Parsed<'p, 2>,
    dequant: AttributeTransform,
    desc: &'p Descriptor,
    parent_id: AttributeId,
    parent_stepper: usize,
}

/// The inputs of one plain group walk.
struct GroupCtx<'p, 'c> {
    members: Vec<GroupMember<'p>>,
    fused: Vec<FusedNormal<'p>>,
    num_values: usize,
    seeds: &'c [CornerIdx],
    replay: Option<Vec<CornerIdx>>,
    record_walk: bool,
}

/// The owned results of one plain group walk.
struct GroupResult {
    members: Vec<(Attribute, AttributeTransform)>,
    fused: Vec<(Attribute, AttributeTransform)>,
    recorded_seq: Option<Vec<CornerIdx>>,
}

/// Builds the group's steppers and fusers and runs one walk.
fn run_plain_group<'p>(ads: &'p PlainDs<'p>, ctx: GroupCtx<'p, '_>) -> Result<GroupResult, Err> {
    let GroupCtx {
        members,
        fused,
        num_values,
        seeds,
        replay,
        record_walk,
    } = ctx;
    let mut steppers: Vec<AnyStepper<'_, PlainDs<'p>>> = Vec::with_capacity(members.len());
    for m in members {
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
            ads,
            f.parent_stepper,
            num_values,
        ));
    }
    let recorded_seq = match replay {
        Some(seq) => {
            decode_group(
                ads,
                seq.iter().copied(),
                &mut steppers,
                &mut fusers,
                num_values,
                None,
            )?;
            None
        }
        None => {
            let mut record_seq = record_walk.then(|| Vec::with_capacity(num_values));
            decode_group(
                ads,
                Traverser::new(ads, seeds.to_vec()),
                &mut steppers,
                &mut fusers,
                num_values,
                record_seq.as_mut(),
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

/// One attribute's correction stream; lazy streams pop in walk order.
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
    /// The correction of rank `k`; lazy streams require consecutive `k`.
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

    /// Drains the stream into a rank-indexed vector.
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

/// One attribute's parsed payload block.
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

    /// Whether the scheme predicts from a decoded position (second wave).
    fn needs_parent(&self) -> bool {
        matches!(
            self.scheme_ty(),
            PredictionSchemeType::MeshNormalPrediction
                | PredictionSchemeType::MeshPredictionForTextureCoordinates
        )
    }
}

/// [`parse_payload`] behind the component-count dispatch.
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

/// Reads a generic attribute's values, widened into the portable i32.
fn read_raw_values<const N: usize>(
    reader: &mut Reader<'_>,
    num_values: usize,
    component_type: ComponentDataType,
) -> Result<Vec<NdVector<N, i32>>, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
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

/// One attribute's decode state through a plain group walk.
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

    /// Decodes this attribute's value of rank `k` at the emitted corner `c`.
    #[inline]
    fn step(&mut self, c: CornerIdx, k: usize, point: PointIdx, record: &[VertexIdx]) {
        // SAFETY: the walk emits at most one corner per distinct vertex, so
        // k < num_values, the length of both the value buffer and any eager
        // correction vector; `point` is a vertex id of the walked structure,
        // below the map length the stepper was constructed with.
        unsafe {
            self.att
                .set_point_att_val_unchecked(point, AttributeValueIdx::from(k));
        }
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

/// [`Stepper`] behind the component-count dispatch.
enum AnyStepper<'p, D: GenericAttributeDs> {
    N1(Stepper<'p, 1, D>),
    N2(Stepper<'p, 2, D>),
    N3(Stepper<'p, 3, D>),
    N4(Stepper<'p, 4, D>),
}

impl<'p, D: GenericAttributeDs> AnyStepper<'p, D> {
    #[inline]
    fn step(&mut self, c: CornerIdx, k: usize, point: PointIdx, record: &[VertexIdx]) {
        match self {
            AnyStepper::N1(s) => s.step(c, k, point, record),
            AnyStepper::N2(s) => s.step(c, k, point, record),
            AnyStepper::N3(s) => s.step(c, k, point, record),
            AnyStepper::N4(s) => s.step(c, k, point, record),
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

/// A normal fused into its parent position's walk: prediction reads no other
/// normals, so one face pass after the walk finalizes every vertex; exact
/// i64 sums make it order-independent.
struct NormalFuser {
    att: Attribute,
    corrections: Vec<NdVector<2, i32>>,
    flips: Vec<bool>,
    zigzagged: bool,
    transform: InverseTransform,
    dequant: AttributeTransform,
    parent_stepper: usize,
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
            corrections: parsed.corrections.materialize(num_values),
            flips: parsed.flips,
            zigzagged,
            transform: parsed.transform,
            dequant,
            parent_stepper,
            rank_of: vec![usize::MAX; ads.vertex_index_bound()],
        }
    }

    /// Records the emit of vertex `v` at rank `k`.
    #[inline]
    fn on_emit(&mut self, v: VertexIdx, k: usize, point: PointIdx) {
        self.att
            .set_point_att_val(point, AttributeValueIdx::from(k));
        self.rank_of[usize::from(v)] = k;
    }

    /// Decodes every normal value after the walk.
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

/// Runs one walk, stepping every member per emitted corner; entries are
/// mapped at emit, and predictions only read already-emitted vertices.
fn decode_group<D: GenericAttributeDs>(
    ads: &D,
    walk: impl Iterator<Item = CornerIdx>,
    steppers: &mut [AnyStepper<'_, D>],
    fusers: &mut [NormalFuser],
    num_values: usize,
    mut record_seq: Option<&mut Vec<CornerIdx>>,
) -> Result<(), Err> {
    let mut record: Vec<VertexIdx> = Vec::with_capacity(num_values);
    let mut k = 0;
    for c in walk {
        if let Some(seq) = record_seq.as_mut() {
            seq.push(c);
        }
        let v = ads.vertex_idx(c);
        let point = PointIdx::from(usize::from(v));
        for s in steppers.iter_mut() {
            s.step(c, k, point, &record);
        }
        for nf in fusers.iter_mut() {
            nf.on_emit(v, k, point);
        }
        record.push(v);
        k += 1;
    }
    if k != num_values {
        return Err(Err::MalformedAttribute(
            "traversal did not reach every attribute vertex",
        ));
    }
    for nf in fusers.iter_mut() {
        let pos = steppers[nf.parent_stepper].att();
        nf.finish_walk(ads, pos);
    }
    Ok(())
}

/// Parses one attribute's portabilization metadata.
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
