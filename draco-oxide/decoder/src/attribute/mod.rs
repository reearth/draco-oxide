//! Attribute decoding: framing (encoder count, domains, descriptors) and the
//! driver that sequences, reverses prediction, inverts transforms, and hands back
//! portable (quantized-integer) attributes.

mod inverse_transform;
mod prediction;
mod sequence;

#[cfg(feature = "dequantize")]
pub(crate) mod dequantize;

use crate::connectivity::{points, Connectivity};
use crate::entropy::{decode_symbols, unzigzag};
use crate::{AttributeTransform, Err};
use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide_core::bit_coder::ByteReader;
use draco_oxide_core::codec::attribute::prediction_scheme::PredictionSchemeType;
use draco_oxide_core::codec::attribute::sequence::Traverser;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::mesh::ds::{AttributeCornerTable, AttributeDS, DS};
use draco_oxide_core::types::{
    AttributeValueIdx, CornerIdx, NdVector, PointIdx, VecCornerIdx, VecPointIdx, Vector, VertexIdx,
};

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
pub(crate) fn decode_attributes<R: ByteReader>(
    reader: &mut R,
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
            // Only depth-first traversal is emitted; MaxPredictionDegree arrives
            // with Google interop.
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

    // One fan walk yields every attribute's vertex map plus, from the union of
    // all seams, the decoder-side point space (the finest common refinement).
    let mut union_seams = vec![false; num_corners];
    for seam in &seams {
        for (u, &s) in union_seams.iter_mut().zip(seam.iter()) {
            *u |= s;
        }
    }
    let mut seam_sets: Vec<&[bool]> = seams.iter().map(|s| s.as_slice()).collect();
    seam_sets.push(&union_seams);
    let mut fans = points::fan_vertices(&conn.corner_table, &seam_sets, num_corners);
    let union_fan = fans.pop().expect("fan_vertices returns one output per set");

    let corner_to_point = points::assign_points(&union_fan);
    let faces: Vec<[PointIdx; 3]> = (0..conn.num_faces)
        .map(|f| {
            [
                corner_to_point[CornerIdx::from(3 * f)],
                corner_to_point[CornerIdx::from(3 * f + 1)],
                corner_to_point[CornerIdx::from(3 * f + 2)],
            ]
        })
        .collect();
    let ds = DS::new(corner_to_point);
    let seeds = sequence::traversal_seeds(conn.num_faces);

    let mut attributes: Vec<Attribute> = Vec::with_capacity(num_atts);
    let mut transforms: Vec<AttributeTransform> = Vec::with_capacity(num_atts);
    for ((desc, seam), fan) in descriptors.iter().zip(seams).zip(fans) {
        let act = AttributeCornerTable::new(&conn.corner_table, VecCornerIdx::from(seam));
        let placeholder = Attribute::new_empty(
            AttributeId::new(desc.uid as usize),
            desc.att_type,
            desc.domain,
            ComponentDataType::I32,
            desc.portable_num_components(),
        );
        let ads = sequence::build_ads(&ds, act, fan, placeholder);
        let seq = Traverser::new(&ads, seeds.clone()).compute_seqeunce();

        let parent = attributes
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::Position);
        let (att, transform) = match desc.portable_num_components() {
            1 => decode_payload::<R, 1>(reader, &ads, &seq, parent, desc)?,
            2 => decode_payload::<R, 2>(reader, &ads, &seq, parent, desc)?,
            3 => decode_payload::<R, 3>(reader, &ads, &seq, parent, desc)?,
            4 => decode_payload::<R, 4>(reader, &ads, &seq, parent, desc)?,
            _ => return Err(Err::MalformedAttribute("unsupported number of components")),
        };
        attributes.push(att);
        transforms.push(transform);
    }

    Ok(DecodedAttributes {
        faces,
        attributes,
        transforms,
    })
}

/// Decodes one attribute's payload: prediction/transform ids, the correction
/// stream, the scheme and transform metadata (in the scheme-dependent order the
/// encoder writes them), the portabilization parameters, and finally the
/// prediction-reversal loop over the traversal sequence.
fn decode_payload<R: ByteReader, const N: usize>(
    reader: &mut R,
    ads: &AttributeDS<'_>,
    sequence: &[CornerIdx],
    parent: Option<&Attribute>,
    desc: &Descriptor,
) -> Result<(Attribute, AttributeTransform), Err>
where
    NdVector<N, i32>: Vector<N, Component = i32> + Portable,
    NdVector<N, f32>: Vector<N, Component = f32> + Portable,
{
    let scheme_ty = prediction::read_scheme_id(reader)?;
    let transform_id = reader.read_u8()?;
    let num_values = sequence.len();

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
    let dequant = read_portabilization::<R, N>(reader, desc.port_type)?;

    // The attribute to fill: one value slot per traversal rank, and a
    // point-to-value map through each point's vertex rank.
    let needs_parent = matches!(
        scheme_ty,
        PredictionSchemeType::MeshNormalPrediction
            | PredictionSchemeType::MeshPredictionForTextureCoordinates
    );
    if needs_parent && parent.is_none() {
        return Err(Err::MalformedAttribute(
            "geometric prediction requires an already decoded position attribute",
        ));
    }
    let parents_ids = if needs_parent {
        vec![parent.unwrap().get_id()]
    } else {
        Vec::new()
    };
    let mut att = Attribute::from_without_removing_duplicates::<NdVector<N, i32>, N>(
        AttributeId::new(desc.uid as usize),
        vec![NdVector::<N, i32>::zero(); num_values],
        desc.att_type,
        desc.domain,
        parents_ids,
    );

    let mut vertex_rank = vec![usize::MAX; ads.num_vertices()];
    for (k, &c) in sequence.iter().enumerate() {
        vertex_rank[usize::from(ads.vertex_idx(c))] = k;
    }
    let gds = ads.global_ds();
    let mut point_map = vec![AttributeValueIdx::from(0); gds.num_points()];
    for c in 0..gds.num_corners() {
        let c = CornerIdx::from(c);
        let rank = vertex_rank[usize::from(ads.vertex_idx(c))];
        if rank == usize::MAX {
            return Err(Err::MalformedAttribute(
                "traversal did not reach every attribute vertex",
            ));
        }
        point_map[usize::from(gds.point_idx(c))] = AttributeValueIdx::from(rank);
    }
    att.set_point_to_att_val_map(Some(VecPointIdx::from(point_map)));

    let parent_refs: Vec<&Attribute> = parent.into_iter().collect();
    let mut predictor = Predictor::<N>::new(&scheme_ty, &parent_refs, ads, flips, orientations)?;

    // Reverse the prediction in traversal order. Predictions only ever read
    // values at already visited vertices, so the partially filled attribute is
    // safe to consult.
    let zigzagged = transform.corrections_are_zigzagged();
    let mut record: Vec<VertexIdx> = Vec::with_capacity(num_values);
    for (k, &c) in sequence.iter().enumerate() {
        let pred = predictor.predict(c, &record, &att);
        record.push(ads.vertex_idx(c));
        let mut corr = corrections[k];
        if zigzagged {
            for i in 0..N {
                *corr.get_mut(i) = unzigzag(*corr.get(i) as u32);
            }
        }
        let orig = transform.compute_original(pred, corr);
        att.unique_vals_as_slice_mut::<NdVector<N, i32>>()[k] = orig;
    }

    Ok((att, dequant))
}

/// Parses the portabilization metadata for `port_type` into the dequantization
/// parameters surfaced through [`AttributeTransform`].
fn read_portabilization<R: ByteReader, const N: usize>(
    reader: &mut R,
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
