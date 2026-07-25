//! Attribute decoding: framing (encoder count, domains, descriptors) and the
//! driver that sequences, reverses prediction, inverts transforms, and hands back
//! portable (quantized-integer) attributes.

mod ds;
mod inverse_transform;
mod prediction;
mod sequence;

#[cfg(feature = "dequantize")]
pub(crate) mod dequantize;

use crate::connectivity::Connectivity;
use crate::entropy::{decode_symbols, unzigzag};
use crate::{AttributeTransform, Err};
use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::attribute::prediction_scheme::mesh_normal_prediction::{
    compute_normal_of_face, sum_to_prediction,
};
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::attribute::sequence::Traverser;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::mesh::ds::{GenericAttributeDs, GenericCornerTable, IdentityDS};
use draco_oxide_core::types::{
    AttributeValueIdx, CornerIdx, NdVector, PointIdx, VecPointIdx, Vector, VertexIdx,
};
use ds::{build_attribute_ds, build_ds, Input};

use inverse_transform::InverseTransform;
use prediction::Predictor;

/// The portabilization type ids on the wire.
const PORT_TO_BITS: u8 = 1;
const PORT_QUANTIZATION_COORDINATE_WISE: u8 = 2;
const PORT_OCTAHEDRAL: u8 = 3;

/// The decoded attribute section: the point-indexed faces plus, per attribute,
/// the portable integer attribute and its dequantization parameters.
pub(crate) struct DecodedAttributes {
    pub faces: Vec<[PointIdx; 3]>,
    pub attributes: Vec<Attribute>,
    pub transforms: Vec<AttributeTransform>,
}

/// One attribute's wire descriptor.
struct Descriptor {
    att_type: AttributeType,
    /// The original component type; the portable representation is always I32.
    /// Consumed once dequantization restores non-f32 original formats.
    #[allow(dead_code)]
    component_type: ComponentDataType,
    num_components: usize,
    uid: u8,
    port_type: u8,
    domain: AttributeDomain,
}

impl Descriptor {
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
    let num_atts = reader.read_u8()? as usize;

    // Framing part 1: per-attribute decoder id, domain, and traversal method.
    let mut domains = Vec::with_capacity(num_atts);
    for _ in 0..num_atts {
        let _decoder_id = reader.read_u8()?;
        let domain = AttributeDomain::read_from(reader)?;
        let traversal = reader.read_u8()?;
        if traversal != 0 {
            return Err(Err::Unimplemented);
        }
        domains.push(domain);
    }

    // Framing part 2: per-attribute descriptors.
    let mut descriptors = Vec::with_capacity(num_atts);
    for domain in domains {
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
        let uid = reader.read_u8()?;
        let port_type = reader.read_u8()?;
        descriptors.push(Descriptor {
            att_type,
            component_type,
            num_components,
            uid,
            port_type,
            domain,
        });
    }

    let num_corners = conn.num_faces * 3;

    // Per-attribute seam edges, in attribute order. The position attribute
    // carries no encoded seams; boundary edges (its only seams) are already
    // seams for every attribute through the corner table itself.
    let mut seams: Vec<Vec<bool>> = Vec::with_capacity(num_atts);
    let mut seam_idx = 0;
    for desc in &descriptors {
        if desc.att_type == AttributeType::Position {
            seams.push(vec![false; num_corners]);
        } else {
            let s = conn
                .attribute_seams
                .get(seam_idx)
                .ok_or(Err::MalformedAttribute(
                    "more non-position attributes than seam streams",
                ))?;
            seam_idx += 1;
            seams.push(s.clone());
        }
    }
    if seam_idx != conn.num_attribute_data {
        return Err(Err::MalformedAttribute(
            "attribute count does not match the connectivity seam streams",
        ));
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

/// Decodes every attribute payload over the shared connectivity `adss`, generic
/// over the attribute data structure so the caller dispatches the identity fast
/// path or the general one. Payload blocks are parsed up front in wire order;
/// decode then runs one lazy traversal per (wave, group), stepping every member
/// attribute at each emitted corner, so a shared sequence is computed exactly
/// once and never materialized.
fn decode_payloads<D: GenericAttributeDs>(
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

    // The wire holds one contiguous block per attribute, so every block is
    // parsed before the first group walk starts.
    let mut parsed: Vec<Option<ParsedPayload>> = Vec::with_capacity(adss.len());
    for (i, desc) in descriptors.iter().enumerate() {
        let num_values = group_num_values[group_ids[i]];
        let payload = match desc.portable_num_components() {
            1 => ParsedPayload::N1(parse_payload::<1>(reader, num_values, desc)?),
            2 => ParsedPayload::N2(parse_payload::<2>(reader, num_values, desc)?),
            3 => ParsedPayload::N3(parse_payload::<3>(reader, num_values, desc)?),
            4 => ParsedPayload::N4(parse_payload::<4>(reader, num_values, desc)?),
            _ => return Err(Err::MalformedAttribute("unsupported number of components")),
        };
        parsed.push(Some(payload));
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
                        .position(|d| d.att_type == AttributeType::Position)
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
            let ads = &adss[rep];
            let num_values = group_num_values[rep];
            let mut steppers: Vec<AnyStepper<'_, D>> = Vec::with_capacity(members.len());
            for &i in &members {
                let parent = if parent_wave {
                    let parent = descriptors[..i].iter().zip(&slots).find_map(|(d, slot)| {
                        if d.att_type == AttributeType::Position {
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
                steppers.push(build_stepper(
                    payload,
                    &descriptors[i],
                    &adss[i],
                    parent,
                    num_values,
                )?);
            }
            let mut fusers: Vec<NormalFuser> = Vec::with_capacity(fused.len());
            for &(i, parent_stepper) in &fused {
                let Some(ParsedPayload::N2(p)) = parsed[i].take() else {
                    unreachable!("fused attributes are 2-component normals");
                };
                let parent_id = AttributeId::new(descriptors[members[parent_stepper]].uid as usize);
                fusers.push(NormalFuser::new(
                    p,
                    &descriptors[i],
                    parent_id,
                    ads,
                    parent_stepper,
                    num_values,
                ));
            }
            let csr = if ads.point_equals_vertex() {
                None
            } else {
                if csrs[rep].is_none() {
                    csrs[rep] = Some(vertex_points_csr(ads));
                }
                csrs[rep].as_ref()
            };
            match recorded_seqs[rep].take() {
                Some(seq) => decode_group(
                    ads,
                    seq.iter().copied(),
                    &mut steppers,
                    &mut fusers,
                    num_values,
                    None,
                    csr,
                )?,
                None => {
                    let mut record_seq = later_wave_members.then(|| Vec::with_capacity(num_values));
                    decode_group(
                        ads,
                        Traverser::new(ads, seeds.to_vec()),
                        &mut steppers,
                        &mut fusers,
                        num_values,
                        record_seq.as_mut(),
                        csr,
                    )?;
                    recorded_seqs[rep] = record_seq;
                }
            }
            let results: Vec<(Attribute, AttributeTransform)> =
                steppers.into_iter().map(AnyStepper::finish).collect();
            for (i, r) in members.into_iter().zip(results) {
                slots[i] = Some(r);
            }
            for ((i, _), fuser) in fused.into_iter().zip(fusers) {
                slots[i] = Some(fuser.finish());
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

/// One attribute's parsed wire payload: prediction/transform ids, the
/// correction stream, the scheme and transform metadata (in the
/// scheme-dependent order the encoder writes them), and the portabilization
/// parameters. Everything the reversal walk consumes.
struct Parsed<const N: usize> {
    scheme_ty: PredictionSchemeType,
    corrections: Vec<NdVector<N, i32>>,
    flips: Vec<bool>,
    orientations: Vec<bool>,
    transform: InverseTransform,
    dequant: AttributeTransform,
}

/// [`Parsed`] behind the component-count dispatch.
enum ParsedPayload {
    N1(Parsed<1>),
    N2(Parsed<2>),
    N3(Parsed<3>),
    N4(Parsed<4>),
}

impl ParsedPayload {
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

/// Parses one attribute's contiguous payload block off the wire.
fn parse_payload<const N: usize>(
    reader: &mut Reader<'_>,
    num_values: usize,
    desc: &Descriptor,
) -> Result<Parsed<N>, Err>
where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
    NdVector<N, f32>: Vector<N, Component = f32> + Portable,
{
    let scheme_ty = prediction::read_scheme_id(reader)?;
    let transform_id = reader.read_u8()?;

    // The correction stream: rANS-coded symbols or raw values.
    let rans_flag = reader.read_u8()?;
    let corrections: Vec<NdVector<N, i32>> = if rans_flag != 0 {
        let symbols = decode_symbols(reader, num_values, N)?;
        symbols
            .chunks_exact(N)
            .map(|chunk| {
                let mut v = NdVector::<N, i32>::zero();
                for (i, &sym) in chunk.iter().enumerate() {
                    *v.get_mut(i) = sym as u32 as i32;
                }
                v
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(num_values);
        for _ in 0..num_values {
            out.push(NdVector::<N, i32>::read_from(reader)?);
        }
        out
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

    // Portabilization (dequantization) parameters come last.
    let dequant = read_portabilization::<N>(reader, desc.port_type)?;

    Ok(Parsed {
        scheme_ty,
        corrections,
        flips,
        orientations,
        transform,
        dequant,
    })
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
    corrections: Vec<NdVector<N, i32>>,
    zigzagged: bool,
    dequant: AttributeTransform,
}

impl<'p, const N: usize, D: GenericAttributeDs> Stepper<'p, N, D>
where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
{
    fn new(
        parsed: Parsed<N>,
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
        )?;
        let zigzagged = parsed.transform.corrections_are_zigzagged();
        Ok(Self {
            att,
            predictor,
            transform: parsed.transform,
            corrections: parsed.corrections,
            zigzagged,
            dequant: parsed.dequant,
        })
    }

    /// Decodes this attribute's value of rank `k` at the emitted corner `c`,
    /// after mapping the corner's fan `points` to `k`.
    #[inline]
    fn step(&mut self, c: CornerIdx, k: usize, points: &[PointIdx], record: &[VertexIdx]) {
        for &p in points {
            self.att.set_point_att_val(p, AttributeValueIdx::from(k));
        }
        // Predictions only ever read values at already visited vertices, so
        // the partially filled attribute is safe to consult.
        let pred = self.predictor.predict(c, record, &self.att);
        let mut corr = self.corrections[k];
        if self.zigzagged {
            for i in 0..N {
                *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
            }
        }
        self.att.unique_vals_as_slice_mut::<NdVector<N, i32>>()[k] =
            self.transform.compute_original(pred, corr);
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
    payload: ParsedPayload,
    desc: &Descriptor,
    ads: &'p D,
    parent: Option<&'p Attribute>,
    num_values: usize,
) -> Result<AnyStepper<'p, D>, Err> {
    Ok(match payload {
        ParsedPayload::N1(p) => AnyStepper::N1(Stepper::new(p, desc, ads, parent, num_values)?),
        ParsedPayload::N2(p) => AnyStepper::N2(Stepper::new(p, desc, ads, parent, num_values)?),
        ParsedPayload::N3(p) => AnyStepper::N3(Stepper::new(p, desc, ads, parent, num_values)?),
        ParsedPayload::N4(p) => AnyStepper::N4(Stepper::new(p, desc, ads, parent, num_values)?),
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
        parsed: Parsed<2>,
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
            corrections: parsed.corrections,
            flips: parsed.flips,
            zigzagged,
            transform: parsed.transform,
            dequant: parsed.dequant,
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
        let mut sums = vec![NdVector::<3, i64>::zero(); self.rank_of.len()];
        for f in 0..ads.num_faces() {
            let c0 = CornerIdx::from(3 * f);
            let pos_c0 = pos.get::<NdVector<3, i32>, 3>(ads.point_idx(c0));
            let face_normal = compute_normal_of_face(ads, pos, c0, pos_c0);
            for t in 0..3 {
                let w = ads.vertex_idx(CornerIdx::from(3 * f + t));
                sums[usize::from(w)] += face_normal;
            }
        }
        for (v, &k) in self.rank_of.iter().enumerate() {
            if k == usize::MAX {
                continue;
            }
            let mut pred = sum_to_prediction::<2>(sums[v]);
            if self.flips.get(k).copied().unwrap_or(false) {
                pred *= -1;
            }
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
            Some((offsets, vertex_points)) => {
                &vertex_points[offsets[usize::from(v)]..offsets[usize::from(v) + 1]]
            }
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

/// Parses the portabilization metadata for `port_type` into the dequantization
/// parameters surfaced through [`AttributeTransform`].
fn read_portabilization<const N: usize>(
    reader: &mut Reader<'_>,
    port_type: u8,
) -> Result<AttributeTransform, Err>
where
    NdVector<N, f32>: Vector<N, Component = f32> + Portable,
{
    match port_type {
        PORT_TO_BITS => Ok(AttributeTransform::None),
        PORT_QUANTIZATION_COORDINATE_WISE => {
            let min = NdVector::<N, f32>::read_from(reader)?;
            let delta_max = f32::read_from(reader)?;
            let bits = reader.read_u8()?;
            Ok(AttributeTransform::Quantized {
                min: (0..N).map(|i| *min.get(i)).collect(),
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
