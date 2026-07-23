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
        let adss = build_attribute_ds(&ds, &conn.corner_table, fans, seams, placeholders);
        let (attributes, transforms) = decode_payloads(reader, &descriptors, &adss, &seeds)?;
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
        let (attributes, transforms) = decode_payloads(reader, &descriptors, &adss, &seeds)?;
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
/// path or the general one. Attributes without interior seams share the
/// position connectivity, so their traversal sequences are identical; the walk
/// runs once and is reused.
fn decode_payloads<D: GenericAttributeDs>(
    reader: &mut Reader<'_>,
    descriptors: &[Descriptor],
    adss: &[D],
    seeds: &[CornerIdx],
) -> Result<(Vec<Attribute>, Vec<AttributeTransform>), Err> {
    let mut shared_seq: Option<Vec<CornerIdx>> = None;
    let mut attributes: Vec<Attribute> = Vec::with_capacity(adss.len());
    let mut transforms: Vec<AttributeTransform> = Vec::with_capacity(adss.len());
    for (desc, ads) in descriptors.iter().zip(adss) {
        let seamless = !ads.has_interior_seams();
        let owned_seq;
        let seq: &[CornerIdx] = if seamless {
            if shared_seq.is_none() {
                shared_seq = Some(Traverser::new(ads, seeds.to_vec()).compute_seqeunce());
            }
            shared_seq.as_deref().unwrap()
        } else {
            owned_seq = Traverser::new(ads, seeds.to_vec()).compute_seqeunce();
            &owned_seq
        };

        let parent = attributes
            .iter()
            .find(|a| a.get_attribute_type() == AttributeType::Position);
        let (att, transform) = match desc.portable_num_components() {
            1 => decode_payload::<1, D>(reader, ads, seq, parent, desc)?,
            2 => decode_payload::<2, D>(reader, ads, seq, parent, desc)?,
            3 => decode_payload::<3, D>(reader, ads, seq, parent, desc)?,
            4 => decode_payload::<4, D>(reader, ads, seq, parent, desc)?,
            _ => return Err(Err::MalformedAttribute("unsupported number of components")),
        };
        attributes.push(att);
        transforms.push(transform);
    }
    Ok((attributes, transforms))
}

/// Decodes one attribute's payload: prediction/transform ids, the correction
/// stream, the scheme and transform metadata (in the scheme-dependent order the
/// encoder writes them), the portabilization parameters, and finally the
/// prediction-reversal loop over the traversal sequence.
fn decode_payload<const N: usize, D: GenericAttributeDs>(
    reader: &mut Reader<'_>,
    ads: &D,
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
    let dequant = read_portabilization::<N>(reader, desc.port_type)?;

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

    let mut vertex_rank = vec![usize::MAX; ads.vertex_index_bound()];
    for (k, &c) in sequence.iter().enumerate() {
        vertex_rank[usize::from(ads.vertex_idx(c))] = k;
    }
    let mut point_map = vec![AttributeValueIdx::from(0); ads.num_points()];
    for c in 0..ads.num_corners() {
        let c = CornerIdx::from(c);
        let rank = vertex_rank[usize::from(ads.vertex_idx(c))];
        if rank == usize::MAX {
            return Err(Err::MalformedAttribute(
                "traversal did not reach every attribute vertex",
            ));
        }
        point_map[usize::from(ads.point_idx(c))] = AttributeValueIdx::from(rank);
    }
    att.set_point_to_att_val_map(Some(VecPointIdx::from(point_map)));

    let parent_refs: Vec<&Attribute> = parent.into_iter().collect();
    let mut predictor = Predictor::<N, D>::new(&scheme_ty, &parent_refs, ads, flips, orientations)?;

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
