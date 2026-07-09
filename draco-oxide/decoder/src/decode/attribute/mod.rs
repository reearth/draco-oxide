//! Attribute decoder.
//!
//! Mirrors `encode/attribute/mod.rs::encode_attributes`. The byte layout this
//! file decodes from is, in order:
//!
//! 1. `u8`  — number of attributes (N).
//! 2. For each attribute (Edgebreaker only):
//!    - `u8` — decoder id (encoder writes `(i as u8).wrapping_sub(1)`).
//!    - `u8` — `AttributeDomain` (Position=0, Corner=1).
//!    - `u8` — `TraversalType` (DepthFirst=0).
//! 3. For each attribute:
//!    - `u8` — attr count in this decoder (always 1).
//!    - `u8` — `AttributeType`.
//!    - `u8` — `ComponentDataType`.
//!    - `u8` — num components.
//!    - `u8` — normalized flag (currently always 0).
//!    - `u8` — unique id.
//!    - `u8` — `PortabilizationType`.
//! 4. For each attribute (the per-attribute encoded payload):
//!    - `u8` — prediction scheme type id.
//!    - `u8` — prediction transform type id.
//!    - …component-type-specific encoded data + metadata.

pub(crate) mod inverse_prediction_transform;
pub(crate) mod portabilization;

use draco_oxide_core::attribute::{
    Attribute, AttributeDomain, AttributeId, AttributeType, ComponentDataType,
};
use draco_oxide_core::bit_coder::ReaderErr;
use draco_oxide_core::corner_table::GenericCornerTable;
use draco_oxide_core::types::{CornerIdx, NdVector, Vector};
use crate::decode::connectivity::DecoderCornerTable;
use crate::decode::entropy::symbol_coding;
use crate::decode::header::Header;
use crate::prelude::ByteReader;
use draco_oxide_core::codec::header::EncoderMethod;

use self::inverse_prediction_transform::{
    InverseTransform, InverseTransformKind, OctahedralOrthogonalInverseTransform,
};
use self::portabilization::{DeportabilizationKind, OctahedralNormal, Quantization};

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Reader error: {0}")]
    Reader(#[from] ReaderErr),
    #[error("Invalid attribute domain id")]
    InvalidAttributeDomain,
    #[error("Invalid attribute type id")]
    InvalidAttributeType,
    #[error("Invalid component data type id")]
    InvalidComponentDataType,
    #[error("Invalid traversal type id: {0}")]
    InvalidTraversalType(u8),
    #[error("Invalid portabilization type id: {0}")]
    InvalidPortabilizationType(u8),
    #[error("Per-attribute prediction scheme not yet implemented: id={0}")]
    PredictionSchemeTodo(u8),
    #[error("RANS encoding flag was {0}, expected 1")]
    RansEncodingDisabled(u8),
    #[error("Inverse prediction transform error: {0}")]
    InverseTransform(#[from] inverse_prediction_transform::Err),
    #[error("Deportabilization error: {0}")]
    Deportabilization(#[from] portabilization::Err),
    #[error("Symbol coding error: {0}")]
    SymbolCoding(#[from] symbol_coding::Err),
    #[error("Rans decoder error: {0}")]
    Rans(#[from] crate::decode::entropy::rans::Err),
    #[error("Unsupported component count: {0}")]
    UnsupportedNumComponents(u8),
    #[error(
        "Symbol stream ran out mid-decode (corner sequence yielded more vertices than symbols)"
    )]
    SymbolStreamUnderrun,
    #[error("Symbol stream had leftover symbols after decode (vertex count mismatch?)")]
    SymbolStreamSurplus,
    #[error("Constrained-multi-parallelogram crease-edge flag count {0} exceeds corner count")]
    CreaseEdgeFlagCountInvalid(usize),
    #[error("Constrained-multi-parallelogram ran out of crease-edge flags mid-decode")]
    CreaseEdgeFlagUnderrun,
    #[error("Attribute core error: {0}")]
    AttributeCore(#[from] draco_oxide_core::attribute::Err),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TraversalType {
    DepthFirst,
    PredictionDegree,
}

impl TraversalType {
    fn from_id(id: u8) -> Result<Self, Err> {
        match id {
            0 => Ok(Self::DepthFirst),
            1 => Ok(Self::PredictionDegree),
            _ => Err(Err::InvalidTraversalType(id)),
        }
    }
}

/// The per-attribute "sequential attribute encoder" id Draco writes for
/// each attribute (Google's `SequentialAttributeEncoderType`):
///   0 → GENERIC      (raw values, no prediction/transform/quantization)
///   1 → INTEGER      (we call it ToBits)
///   2 → QUANTIZATION (coordinate-wise)
///   3 → NORMALS      (octahedral)
#[derive(Debug, Clone, Copy)]
pub(crate) enum PortabilizationType {
    /// Google's `SEQUENTIAL_ATTRIBUTE_ENCODER_GENERIC`. The base
    /// `SequentialAttributeDecoder`: attribute values are stored verbatim
    /// in their original component format, in mesh-traversal order, with
    /// no prediction scheme, transform, or entropy coding. Used e.g. by
    /// producers that leave positions as raw f32 and only compress
    /// connectivity.
    Generic,
    ToBits,
    QuantizationCoordinateWise,
    OctahedralQuantization,
}

impl PortabilizationType {
    fn from_id(id: u8) -> Result<Self, Err> {
        // IDs match Google's `SequentialAttributeEncoderType`
        // (compression_shared.h) — also `encode/attribute/portabilization/mod.rs`.
        match id {
            0 => Ok(Self::Generic),
            1 => Ok(Self::ToBits),
            2 => Ok(Self::QuantizationCoordinateWise),
            3 => Ok(Self::OctahedralQuantization),
            _ => Err(Err::InvalidPortabilizationType(id)),
        }
    }
}

/// Per-attribute metadata read from the bitstream before the actual data.
#[allow(dead_code)] // Read during the per-attribute decode pipeline.
pub(crate) struct AttributeMeta {
    pub decoder_id: Option<u8>,
    pub domain: AttributeDomain,
    pub traversal: TraversalType,

    pub attribute_type: AttributeType,
    pub component_type: ComponentDataType,
    pub num_components: u8,
    pub normalized: u8,
    pub unique_id: u8,
    pub portabilization: PortabilizationType,
}

#[derive(Debug, Clone)]
pub struct Config {}

impl crate::prelude::ConfigType for Config {
    fn default() -> Self {
        Self {}
    }
}

/// Quantized i32 positions indexed by corner-table vertex ID. Threaded
/// from the position decoder into the normal/UV decoders so they can
/// run prediction in the int-quantized domain without losing
/// precision via dequantize → re-quantize.
pub(crate) type CtVertexQuantizedPositions = Vec<[i32; 3]>;

/// `(decoded_attribute, optional_ct_vertex_indexed_positions,
///   effective_decoder_id)` — return shape of `decode_one_attribute`
/// and the position-attribute decoder.
pub(crate) type DecodedAttribute = (Attribute, Option<CtVertexQuantizedPositions>, Option<u8>);


/// Reads all attribute metadata then decodes each attribute.
///
/// When an unsupported attribute is encountered, decoding stops and
/// returns the attributes decoded so far so that downstream consumers
/// can still use the positions. A `DecodeWarning::AttributeSkipped`
/// is appended to `warnings` so callers can detect the partial
/// decode.
pub(crate) fn decode_attributes<R: ByteReader>(
    reader: &mut R,
    header: &Header,
    corner_table: &DecoderCornerTable,
    attribute_corner_tables: &[crate::decode::connectivity::DecoderAttributeCornerTable],
    start_corners: &[CornerIdx],
    num_position_vertices: usize,
    cfg: Config,
    warnings: &mut Vec<crate::decode::DecodeWarning>,
) -> Result<Vec<Attribute>, Err> {
    Ok(decode_attributes_with_meta(
        reader,
        header,
        corner_table,
        attribute_corner_tables,
        start_corners,
        num_position_vertices,
        cfg,
        warnings,
    )?
    .into_iter()
    .map(|(att, _)| att)
    .collect())
}

/// Like [`decode_attributes`] but also returns each attribute's
/// `decoder_id`. `None` means the attribute is decoded against the
/// universal corner table (position); `Some(idx)` indexes into
/// `attribute_corner_tables`.
pub(crate) fn decode_attributes_with_meta<R: ByteReader>(
    reader: &mut R,
    header: &Header,
    corner_table: &DecoderCornerTable,
    attribute_corner_tables: &[crate::decode::connectivity::DecoderAttributeCornerTable],
    start_corners: &[CornerIdx],
    num_position_vertices: usize,
    _cfg: Config,
    warnings: &mut Vec<crate::decode::DecodeWarning>,
) -> Result<Vec<(Attribute, Option<u8>)>, Err> {
    let metas = read_metadata(reader, header)?;
    let mut out: Vec<(Attribute, Option<u8>)> = Vec::with_capacity(metas.len());
    // Auxiliary buffer of QUANTIZED positions (i32, in the encoder's
    // [0, max_quant] range) INDEXED BY CORNER-TABLE VERTEX ID — not the
    // compacted attribute index. `MeshNormalPrediction` works in the
    // i32 quantized domain, so passing the original i32 values (rather
    // than dequantized f32 + re-quantize) avoids a precision-losing
    // round trip.
    let mut positions_by_ct_vertex: Option<Vec<[i32; 3]>> = None;
    for (attribute_index, meta) in metas.iter().enumerate() {
        let position_parent = out
            .iter()
            .map(|(a, _)| a)
            .find(|a| a.get_attribute_type() == AttributeType::Position);
        // Pick the corner table this attribute should be decoded against.
        // `decoder_id` indexes into `attribute_corner_tables` (encoder
        // wrote `(i as u8).wrapping_sub(1)`, so 0xFF = use universal
        // table for the first attribute = position).
        let attr_table = match meta.decoder_id {
            Some(idx) if (idx as usize) < attribute_corner_tables.len() => {
                Some(&attribute_corner_tables[idx as usize])
            }
            _ => None,
        };
        match decode_one_attribute(
            reader,
            meta,
            corner_table,
            attr_table,
            start_corners,
            num_position_vertices,
            position_parent,
            positions_by_ct_vertex.as_deref(),
        ) {
            Ok((att, ct_indexed, effective_decoder_id)) => {
                if att.get_attribute_type() == AttributeType::Position {
                    positions_by_ct_vertex = ct_indexed;
                }
                out.push((att, effective_decoder_id));
            }
            // Best-effort: if a non-position attribute trips an
            // unimplemented decode path (oct transforms, oct port,
            // 2-component layouts, MeshNormalPrediction, etc.), record
            // a warning and return what we've decoded so far rather
            // than failing the whole mesh. The caller still gets
            // correctly-decoded earlier attributes. After this, the
            // byte stream is in an undefined state — we can't continue
            // to the next attribute.
            Err(
                reason @ (Err::PredictionSchemeTodo(_)
                | Err::UnsupportedNumComponents(_)
                | Err::InverseTransform(inverse_prediction_transform::Err::OctahedralTodo)),
            ) => {
                warnings.push(crate::decode::DecodeWarning::AttributeSkipped {
                    attribute_index,
                    attribute_type: meta.attribute_type,
                    reason: reason.to_string(),
                });
                return Ok(out);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Decodes a single attribute into an `Attribute`, mirroring
/// `encode/attribute/attribute_encoder.rs::encode_portabilized` byte-for-byte
/// on the read side and applying the inverse pipeline:
///
///   symbols → from_positive_i32 → inverse_transform(corr, prediction)
///                              → quantized i32 (per vertex)
///                              → deportabilize → original f32 attribute
///
/// `start_corners` are the seeds for the per-attribute traverser (one per
/// connected component, recorded during `decode_connectivity`).
/// Returns `(attribute, optional_ct_vertex_indexed_positions,
/// effective_decoder_id)`. The second value is `Some` only for the
/// Position branch; downstream normal/UV decoders use it to look up
/// positions by corner-table vertex ID rather than the compacted
/// attribute index. The third value is the corner table actually used
/// for indexing this attribute's value buffer: `None` = universal
/// corner table, `Some(idx)` = `attribute_corner_tables[idx]`. Some
/// branches OVERRIDE the bitstream's `decoder_id` (e.g. UV's
/// MeshPredictionForTextureCoordinates currently always uses the
/// universal table), so callers that index back into the value
/// buffer (`decode_to_raw`) must use this effective ID rather than
/// `meta.decoder_id`.
/// Decodes attributes for a SEQUENTIAL mesh. Values are stored in plain point
/// order (Google's `LinearSequencer`) with delta prediction — there is no
/// corner table, so no mesh prediction (parallelogram / geometric-normal /
/// texcoord-portable) or per-attribute seam tables. Returns each attribute in
/// point order; the `Option<u8>` is always `None` (point→value is identity).
pub(crate) fn decode_attributes_sequential<R: ByteReader>(
    reader: &mut R,
    header: &Header,
    num_points: usize,
) -> Result<Vec<(Attribute, Option<u8>)>, Err> {
    let _header = header;
    let metas = read_metadata_sequential(reader)?;
    let mut out = Vec::with_capacity(metas.len());
    for meta in &metas {
        let n = meta.num_components as usize;
        let att = if matches!(meta.portabilization, PortabilizationType::Generic) {
            match n {
                1 => decode_generic_linear::<R, 1>(reader, meta, num_points)?,
                2 => decode_generic_linear::<R, 2>(reader, meta, num_points)?,
                3 => decode_generic_linear::<R, 3>(reader, meta, num_points)?,
                4 => decode_generic_linear::<R, 4>(reader, meta, num_points)?,
                _ => return Err(Err::UnsupportedNumComponents(meta.num_components)),
            }
        } else {
            let pred_scheme_id = reader.read_u8()?;
            let xform_kind = InverseTransformKind::from_id(reader.read_u8()?)?;
            match (meta.attribute_type, meta.portabilization) {
                (AttributeType::Normal, PortabilizationType::OctahedralQuantization) => {
                    decode_normal_linear(reader, meta, num_points, pred_scheme_id)?
                }
                _ => match n {
                    1 => decode_quantized_linear::<R, 1>(reader, meta, num_points, xform_kind)?,
                    2 => decode_quantized_linear::<R, 2>(reader, meta, num_points, xform_kind)?,
                    3 => decode_quantized_linear::<R, 3>(reader, meta, num_points, xform_kind)?,
                    4 => decode_quantized_linear::<R, 4>(reader, meta, num_points, xform_kind)?,
                    _ => return Err(Err::UnsupportedNumComponents(meta.num_components)),
                },
            }
        };
        out.push((att, None));
    }
    Ok(out)
}

/// Reads the per-attribute metadata for a SEQUENTIAL mesh, matching Google's
/// base `AttributesDecoder::DecodeAttributesDecoderData` +
/// `SequentialAttributeDecodersController` layout (v2.2):
///   u8  num_attributes_decoders   (PointCloudDecoder wrapper; expect 1)
///   varint num_attributes
///   per attr: att_type(u8) data_type(u8) num_components(u8) normalized(u8) unique_id(varint)
///   per attr: decoder_type(u8)    (GENERIC/INTEGER/QUANTIZATION/NORMALS)
fn read_metadata_sequential<R: ByteReader>(reader: &mut R) -> Result<Vec<AttributeMeta>, Err> {
    use draco_oxide_core::utils::bit_coder::leb128_read;

    let _num_attributes_decoders = reader.read_u8()?;
    let num_attributes = leb128_read(reader)? as usize;

    // GeometryAttribute descriptors.
    let mut descs: Vec<(AttributeType, ComponentDataType, u8, u8, u8)> =
        Vec::with_capacity(num_attributes);
    for _ in 0..num_attributes {
        let attribute_type =
            AttributeType::read_from(reader).map_err(|_| Err::InvalidAttributeType)?;
        let component_type =
            ComponentDataType::read_from(reader).map_err(|_| Err::InvalidComponentDataType)?;
        let num_components = reader.read_u8()?;
        let normalized = reader.read_u8()?;
        let unique_id = leb128_read(reader)? as u8;
        descs.push((
            attribute_type,
            component_type,
            num_components,
            normalized,
            unique_id,
        ));
    }

    // Sequential decoder types (separate trailing loop).
    let mut metas = Vec::with_capacity(num_attributes);
    for (attribute_type, component_type, num_components, normalized, unique_id) in descs {
        let portabilization = PortabilizationType::from_id(reader.read_u8()?)?;
        metas.push(AttributeMeta {
            decoder_id: None,
            domain: AttributeDomain::Position,
            traversal: TraversalType::DepthFirst,
            attribute_type,
            component_type,
            num_components,
            normalized,
            unique_id,
            portabilization,
        });
    }
    Ok(metas)
}

/// GENERIC sequential attribute: raw values, one per point, in order.
fn decode_generic_linear<R: ByteReader, const N: usize>(
    reader: &mut R,
    meta: &AttributeMeta,
    num_points: usize,
) -> Result<Attribute, Err>
where
    NdVector<N, f32>: Vector<N, Component = f32>,
{
    let mut data: Vec<NdVector<N, f32>> = Vec::with_capacity(num_points);
    for _ in 0..num_points {
        let mut nd = <NdVector<N, f32> as Vector<N>>::zero();
        for j in 0..N {
            *nd.get_mut(j) = read_component_as_f32(reader, meta.component_type)?;
        }
        data.push(nd);
    }
    Ok(Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    ))
}

/// INTEGER/QUANTIZATION sequential attribute: delta-predicted in point order
/// (no mesh prediction for sequential meshes), then dequantized.
fn decode_quantized_linear<R: ByteReader, const N: usize>(
    reader: &mut R,
    meta: &AttributeMeta,
    num_points: usize,
    xform_kind: InverseTransformKind,
) -> Result<Attribute, Err>
where
    NdVector<N, f32>: Vector<N, Component = f32>,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let symbols = read_corrections(num_points * N, N, reader)?;
    let inverse_xform = InverseTransform::read(reader, xform_kind)?;
    let dequant = match meta.portabilization {
        PortabilizationType::QuantizationCoordinateWise => Some(Quantization::read(reader, N)?),
        PortabilizationType::ToBits => None,
        _ => return Err(Err::InvalidPortabilizationType(0)),
    };

    let mut data: Vec<NdVector<N, f32>> = Vec::with_capacity(num_points);
    let mut last = [0i32; N];
    let mut tmp = vec![0f32; N];
    for i in 0..num_points {
        let mut corr = [0i32; N];
        for k in 0..N {
            corr[k] = symbols[i * N + k] as i32;
        }
        let mut value = [0i32; N];
        inverse_xform.inverse_n(&corr, &last, &mut value);
        last = value;
        let mut nd = <NdVector<N, f32> as Vector<N>>::zero();
        match &dequant {
            Some(q) => {
                q.dequantize_into(&value, &mut tmp);
                for (j, &val) in tmp.iter().enumerate() {
                    *nd.get_mut(j) = val;
                }
            }
            None => {
                for (j, &v) in value.iter().enumerate() {
                    *nd.get_mut(j) = v as f32;
                }
            }
        }
        data.push(nd);
    }
    Ok(Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    ))
}

/// NORMALS sequential attribute: octahedral delta in point order. Output is
/// 3D unit-vector f32 normals.
fn decode_normal_linear<R: ByteReader>(
    reader: &mut R,
    meta: &AttributeMeta,
    num_points: usize,
    pred_scheme_id: u8,
) -> Result<Attribute, Err> {
    if pred_scheme_id == MESH_GEOMETRIC_NORMAL_PREDICTION_ID {
        // Geometric-normal prediction needs a corner table (positions); a
        // sequential mesh has none, so this combination shouldn't occur.
        return Err(Err::PredictionSchemeTodo(pred_scheme_id));
    }
    // Byte order matches the edgebreaker normal path: corrections, then the
    // octahedral transform metadata, then the quantization metadata.
    let symbols = read_corrections(num_points * 2, 2, reader)?;
    let inverse_xform = OctahedralOrthogonalInverseTransform::read(reader)?;
    let dequant = OctahedralNormal::read(reader)?;

    let mut data: Vec<NdVector<3, f32>> = Vec::with_capacity(num_points);
    let mut last = [0i32; 2];
    for i in 0..num_points {
        let corr = [symbols[i * 2] as i32, symbols[i * 2 + 1] as i32];
        let value = inverse_xform.inverse(&corr, &last);
        last = value;
        let n3 = dequant.dequantize(&value);
        let mut nd = <NdVector<3, f32> as Vector<3>>::zero();
        for (j, &v) in n3.iter().enumerate() {
            *nd.get_mut(j) = v;
        }
        data.push(nd);
    }
    Ok(Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    ))
}

fn decode_one_attribute<R: ByteReader>(
    reader: &mut R,
    meta: &AttributeMeta,
    corner_table: &DecoderCornerTable,
    attr_table: Option<&crate::decode::connectivity::DecoderAttributeCornerTable>,
    start_corners: &[CornerIdx],
    num_position_vertices: usize,
    position_parent: Option<&Attribute>,
    positions_by_ct_vertex: Option<&[[i32; 3]]>,
) -> Result<DecodedAttribute, Err> {
    // GENERIC encoder (Google's base SequentialAttributeDecoder): values
    // are stored raw in their original format with no prediction scheme,
    // transform, or entropy coding — so it does NOT emit the
    // pred_scheme/transform/rans header bytes the other encoders do. Branch
    // before reading them.
    if matches!(meta.portabilization, PortabilizationType::Generic) {
        let n_components = meta.num_components as usize;
        let att = match n_components {
            2 => decode_generic_attribute::<R, 2>(reader, meta, corner_table, num_position_vertices)?,
            3 => decode_generic_attribute::<R, 3>(reader, meta, corner_table, num_position_vertices)?,
            4 => decode_generic_attribute::<R, 4>(reader, meta, corner_table, num_position_vertices)?,
            _ => return Err(Err::UnsupportedNumComponents(meta.num_components)),
        };
        return Ok((att, None, None));
    }

    // ── 1-2: prediction method + transform-type bytes ───────────────────
    // The third byte (the "compressed"/rANS flag) is read by each per-attribute
    // decoder right before its corrections, so the raw-vs-entropy-coded branch
    // lives next to where the values are consumed.
    let pred_scheme_id = reader.read_u8()?;
    let xform_kind = InverseTransformKind::from_id(reader.read_u8()?)?;

    let n_components = meta.num_components as usize;

    // Dispatch on (attribute_type, port_kind). The encoder bundles attr
    // type with a specific (prediction_scheme + transform + port) triple;
    // we mirror those triples here with one branch per real combination.
    match (meta.attribute_type, meta.portabilization) {
        // Position: N=3, MeshParallelogramPrediction + WrappedDifference
        // + QuantizationCoordinateWise. Verified compatible with Google
        // for tetrahedron/sphere/torus/bunny.
        (AttributeType::Position, PortabilizationType::QuantizationCoordinateWise)
            if n_components == 3 =>
        {
            // Positions are never seamed (decoded against the universal table).
            let (att, ct_idx) = decode_quantized_attribute::<R, _, 3>(
                reader,
                meta,
                corner_table,
                num_position_vertices,
                pred_scheme_id,
                xform_kind,
                /* return_ct_indexed */ true,
            )?;
            Ok((att, ct_idx, None))
        }
        // TextureCoordinate: N=2, MeshPredictionForTextureCoordinates (id=5)
        // + WrappedDifference + QuantizationCoordinateWise — the Google
        // default. The encoder side picks a 3D-triangle-plane prediction
        // (closest of two sign-flipped variants) and stores one
        // orientation bit per visited vertex.
        (AttributeType::TextureCoordinate, PortabilizationType::QuantizationCoordinateWise)
            if n_components == 2
                && pred_scheme_id == MESH_PREDICTION_FOR_TEXTURE_COORDINATES_ID =>
        {
            // Thread the per-attribute corner table through so seam-split UVs
            // (different vertex count from positions) decode correctly: the
            // UV traversal/indexing uses the attr table while position lookups
            // for the predictor stay on the universal table.
            let att = decode_uv_attribute(
                reader,
                meta,
                corner_table,
                attr_table,
                start_corners,
                num_position_vertices,
                xform_kind,
                positions_by_ct_vertex,
            )?;
            let effective = if attr_table.is_some() {
                meta.decoder_id
            } else {
                None
            };
            Ok((att, None, effective))
        }
        // TextureCoordinate fallback for parallelogram-style predictions
        // (id != 5). Routes through the per-attribute corner table when the
        // UVs are seamed.
        (AttributeType::TextureCoordinate, PortabilizationType::QuantizationCoordinateWise)
            if n_components == 2 =>
        {
            let (att, effective) = decode_quantized_seamed::<R, 2>(
                reader,
                meta,
                corner_table,
                attr_table,
                num_position_vertices,
                pred_scheme_id,
                xform_kind,
            )?;
            Ok((att, None, effective))
        }
        // Normal: stored as N=2 oct-quantized i32 in the symbol stream
        // even though the metadata says num_components=3 (the output
        // dim — what the consumer of the Attribute sees). Output is 3D
        // unit normals.
        (AttributeType::Normal, PortabilizationType::OctahedralQuantization) => {
            let att = decode_normal_attribute(
                reader,
                meta,
                corner_table,
                attr_table,
                num_position_vertices,
                pred_scheme_id,
                position_parent,
                positions_by_ct_vertex,
            )?;
            // Effective table = whatever we actually passed to the
            // decoder (Some when seamed, None when not).
            let effective = if attr_table.is_some() {
                meta.decoder_id
            } else {
                None
            };
            Ok((att, None, effective))
        }
        // Color: N=3 (RGB) or N=4 (RGBA), QuantizationCoordinateWise
        // with DeltaPrediction. Same decode path as Position, just
        // parameterized over N. The encoder side defaults Color to
        // QuantizationCoordinateWise + WrappedDifference + 11-bit.
        (AttributeType::Color, PortabilizationType::QuantizationCoordinateWise)
            if n_components == 3 =>
        {
            let (att, effective) = decode_quantized_seamed::<R, 3>(
                reader,
                meta,
                corner_table,
                attr_table,
                num_position_vertices,
                pred_scheme_id,
                xform_kind,
            )?;
            Ok((att, None, effective))
        }
        (AttributeType::Color, PortabilizationType::QuantizationCoordinateWise)
            if n_components == 4 =>
        {
            let (att, effective) = decode_quantized_seamed::<R, 4>(
                reader,
                meta,
                corner_table,
                attr_table,
                num_position_vertices,
                pred_scheme_id,
                xform_kind,
            )?;
            Ok((att, None, effective))
        }
        // Any other integer/quantized attribute, for ANY component count —
        // TANGENT (N=4), VEC4 COLOR (N=4, common on skinned meshes), and
        // custom/generic attrs (ToBits passthrough). Same integer +
        // prediction (parallelogram/multi/constrained/delta) decode as
        // Position/Color, parameterized over N; `decode_quantized_seamed`
        // dequantizes for QuantizationCoordinateWise and passes ints through
        // for ToBits. Without this, an unhandled attribute trips the catch-all
        // below and (because the byte stream is then desynced) also drops
        // every attribute encoded after it — e.g. COLOR/TEXCOORD → an
        // untextured or black mesh.
        (_, PortabilizationType::QuantizationCoordinateWise | PortabilizationType::ToBits) => {
            let (att, effective) = match n_components {
                2 => decode_quantized_seamed::<R, 2>(
                    reader,
                    meta,
                    corner_table,
                    attr_table,
                    num_position_vertices,
                    pred_scheme_id,
                    xform_kind,
                )?,
                3 => decode_quantized_seamed::<R, 3>(
                    reader,
                    meta,
                    corner_table,
                    attr_table,
                    num_position_vertices,
                    pred_scheme_id,
                    xform_kind,
                )?,
                4 => decode_quantized_seamed::<R, 4>(
                    reader,
                    meta,
                    corner_table,
                    attr_table,
                    num_position_vertices,
                    pred_scheme_id,
                    xform_kind,
                )?,
                _ => return Err(Err::UnsupportedNumComponents(meta.num_components)),
            };
            Ok((att, None, effective))
        }
        _ => Err(Err::UnsupportedNumComponents(meta.num_components)),
    }
}

/// Decode flow for N-component attributes that use:
/// `MeshParallelogramPrediction` (or fall-back to last-decoded) +
/// `WrappedDifference` + `QuantizationCoordinateWise`.
///
/// Used by Position (N=3) and TextureCoordinate (N=2). Both attribute
/// types share the same byte layout once you parameterize over N.
/// Reads the per-attribute "compressed" flag and then the integer
/// corrections. Mirrors Google's `SequentialIntegerAttributeDecoder::
/// DecodeIntegerValues`: flag != 0 → rANS symbol-coded; flag == 0 → raw
/// fixed-width little-endian values (a leading byte-width, then one value per
/// symbol). Either way the result is the stream of non-negative "zigzag"
/// symbols the inverse prediction transform expects. Used by every
/// integer/quantized attribute path (position, UV, color, normal).
fn read_corrections<R: ByteReader>(
    num_symbols: usize,
    num_components: usize,
    reader: &mut R,
) -> Result<Vec<u64>, Err> {
    let compressed = reader.read_u8()?;
    if compressed != 0 {
        return Ok(symbol_coding::decode_symbols(
            num_symbols,
            num_components,
            reader,
        )?);
    }
    // Uncompressed: a single byte-width followed by `num_symbols` raw values,
    // each `num_bytes` little-endian bytes.
    let num_bytes = reader.read_u8()? as usize;
    let mut out = Vec::with_capacity(num_symbols);
    for _ in 0..num_symbols {
        let mut v: u32 = 0;
        for b in 0..num_bytes {
            v |= (reader.read_u8()? as u32) << (8 * b);
        }
        out.push(v as u64);
    }
    Ok(out)
}

/// Dispatches a quantized/integer attribute decode to either the universal
/// corner table or the attribute's own (seam-split) corner table, returning
/// the attribute plus the effective decoder id (`Some(decoder_id)` when the
/// per-attribute table was used, so `decode_to_raw` indexes values through it).
/// Used for Color and parallelogram-predicted TexCoords/Custom — whichever may
/// be seamed.
#[allow(clippy::type_complexity)]
fn decode_quantized_seamed<R: ByteReader, const N: usize>(
    reader: &mut R,
    meta: &AttributeMeta,
    corner_table: &DecoderCornerTable,
    attr_table: Option<&crate::decode::connectivity::DecoderAttributeCornerTable>,
    num_position_vertices: usize,
    pred_scheme_id: u8,
    xform_kind: InverseTransformKind,
) -> Result<(Attribute, Option<u8>), Err>
where
    NdVector<N, f32>: Vector<N, Component = f32>,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    match attr_table {
        Some(t) => {
            let (att, _) = decode_quantized_attribute::<R, _, N>(
                reader,
                meta,
                t,
                t.num_vertices,
                pred_scheme_id,
                xform_kind,
                false,
            )?;
            Ok((att, meta.decoder_id))
        }
        None => {
            let (att, _) = decode_quantized_attribute::<R, _, N>(
                reader,
                meta,
                corner_table,
                num_position_vertices,
                pred_scheme_id,
                xform_kind,
                false,
            )?;
            Ok((att, None))
        }
    }
}

fn decode_quantized_attribute<
    R: ByteReader,
    CT: draco_oxide_core::corner_table::GenericCornerTable,
    const N: usize,
>(
    reader: &mut R,
    meta: &AttributeMeta,
    corner_table: &CT,
    num_attr_values: usize,
    pred_scheme_id: u8,
    xform_kind: InverseTransformKind,
    return_ct_indexed: bool,
) -> Result<(Attribute, Option<CtVertexQuantizedPositions>), Err>
where
    NdVector<N, f32>: Vector<N, Component = f32>,
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    let num_symbols = num_attr_values * N;
    let symbols = read_corrections(num_symbols, N, reader)?;

    let scheme = PositionPredictionScheme::from_id(pred_scheme_id)?;

    // Byte order mirrors Google's `SequentialIntegerAttributeDecoder`:
    // after the entropy-coded corrections, the prediction scheme reads its
    // own metadata (`decodePredictionData`) BEFORE the transform's min/max.
    // For constrained-multi that metadata is the crease-edge flags; the
    // other schemes have none.
    let crease_edges = if scheme == PositionPredictionScheme::ConstrainedMultiParallelogram {
        Some(read_crease_edge_flags(reader, corner_table.num_corners())?)
    } else {
        None
    };

    let inverse_xform = InverseTransform::read(reader, xform_kind)?;
    let port_kind = match meta.portabilization {
        PortabilizationType::ToBits => DeportabilizationKind::ToBits,
        PortabilizationType::QuantizationCoordinateWise => {
            DeportabilizationKind::QuantizationCoordinateWise
        }
        PortabilizationType::OctahedralQuantization => {
            DeportabilizationKind::OctahedralQuantization
        }
        // GENERIC is dispatched to `decode_generic_attribute` before we
        // ever reach the quantized path; never hit here.
        PortabilizationType::Generic => return Err(Err::InvalidPortabilizationType(0)),
    };
    let dequant = match port_kind {
        DeportabilizationKind::QuantizationCoordinateWise => Some(Quantization::read(reader, N)?),
        DeportabilizationKind::ToBits => None,
        // Octahedral is only valid for the dedicated normal path; the
        // dispatch in `decode_one_attribute` routes it there, so this is
        // never reached in practice — return an error rather than panic
        // if a malformed stream ever lands here.
        DeportabilizationKind::OctahedralQuantization => {
            return Err(Err::InvalidPortabilizationType(3))
        }
    };

    // The attribute traversal order must match the method the encoder used
    // (recorded per attribute in `meta.traversal`). Depth-first is the
    // default; max-prediction-degree is used at higher compression levels
    // (and pairs with constrained-multi-parallelogram).
    let sequence = match meta.traversal {
        TraversalType::DepthFirst => {
            draco_oxide_core::codec::attribute::sequence::compute_sequence_depth_first(corner_table)
        }
        TraversalType::PredictionDegree => {
            draco_oxide_core::codec::attribute::sequence::compute_sequence_max_prediction_degree(corner_table)
        }
    };

    let buf_len = corner_table.num_vertices().max(num_attr_values);
    let mut partial: Vec<[i32; N]> = vec![[0; N]; buf_len];
    let mut visited = vec![false; buf_len];
    let mut last_decoded: [i32; N] = [0; N];
    let mut symbol_idx = 0usize;
    // Per-context running cursors into the crease-edge flag arrays
    // (constrained-multi only). Indexed by (num_parallelograms - 1).
    let mut crease_pos = [0usize; MAX_NUM_PARALLELOGRAMS];

    for c in &sequence {
        let v = corner_table.vertex_idx(*c);
        let v_idx = usize::from(v);
        if visited[v_idx] {
            continue;
        }

        let pred = match scheme {
            PositionPredictionScheme::Delta => last_decoded,
            PositionPredictionScheme::Parallelogram => {
                compute_parallelogram_pred::<_, N>(corner_table, *c, &visited, &partial)
                    .unwrap_or(last_decoded)
            }
            PositionPredictionScheme::MultiParallelogram => {
                compute_multi_parallelogram_pred::<_, N>(
                    corner_table,
                    *c,
                    &visited,
                    &partial,
                    last_decoded,
                )
            }
            PositionPredictionScheme::ConstrainedMultiParallelogram => {
                compute_constrained_multi_parallelogram_pred::<_, N>(
                    corner_table,
                    *c,
                    &visited,
                    &partial,
                    last_decoded,
                    crease_edges.as_ref().expect("crease flags read for constrained-multi"),
                    &mut crease_pos,
                )?
            }
        };

        if symbol_idx + N > symbols.len() {
            return Err(Err::SymbolStreamUnderrun);
        }
        let mut corr = [0i32; N];
        for i in 0..N {
            corr[i] = symbols[symbol_idx + i] as i32;
        }
        symbol_idx += N;

        let mut value = [0i32; N];
        inverse_xform.inverse_n(&corr, &pred, &mut value);
        partial[v_idx] = value;
        last_decoded = value;
        visited[v_idx] = true;
    }

    if symbol_idx != symbols.len() {
        return Err(Err::SymbolStreamSurplus);
    }

    // Deportabilize. Output vertices in vertex-id order, written
    // straight into the final `Vec<NdVector<N, f32>>` so we don't
    // pay for a flat `Vec<f32>` intermediate + element-wise repack.
    let mut data: Vec<NdVector<N, f32>> = Vec::with_capacity(num_attr_values);
    let mut tmp = vec![0f32; N];
    // For an integer-passthrough (ToBits) attribute flagged `normalized` in the
    // bitstream (glTF normalized COLOR/etc.), the stored ints map to [0,1]
    // (unsigned) / [-1,1] (signed). Emit the normalized float so consumers get
    // the right range — otherwise e.g. a u16 COLOR comes out as 65535, which
    // clamps wrong. Non-normalized ints (JOINTS indices) pass through.
    // Normalize when the bitstream flags the attribute `normalized`, OR when
    // it's a COLOR stored as integers (glTF colors are semantically [0,1] —
    // Draco doesn't always carry the normalized bit, but a u8/u16 COLOR is
    // always normalized). Yields [0,1] (unsigned) / [-1,1] (signed) instead of
    // raw ints like 65535.
    let normalize_scale: Option<f32> = if dequant.is_none()
        && (meta.normalized != 0 || meta.attribute_type == AttributeType::Color)
    {
        use draco_oxide_core::attribute::ComponentDataType::*;
        match meta.component_type {
            U8 => Some(1.0 / 255.0),
            U16 => Some(1.0 / 65535.0),
            U32 => Some(1.0 / 4294967295.0),
            I8 => Some(1.0 / 127.0),
            I16 => Some(1.0 / 32767.0),
            _ => None,
        }
    } else {
        None
    };
    for (i, v) in partial.iter().enumerate() {
        if !visited[i] {
            continue;
        }
        match &dequant {
            Some(q) => q.dequantize_into(v, &mut tmp),
            None => match normalize_scale {
                Some(s) => {
                    for j in 0..N {
                        tmp[j] = v[j] as f32 * s;
                    }
                }
                None => {
                    for j in 0..N {
                        tmp[j] = v[j] as f32;
                    }
                }
            },
        }
        let mut nd = <NdVector<N, f32> as Vector<N>>::zero();
        for (j, &val) in tmp.iter().enumerate() {
            *nd.get_mut(j) = val;
        }
        data.push(nd);
    }

    // Optionally produce a vertex-indexed (= corner-table indexed)
    // copy of the QUANTIZED i32 positions for downstream Normal
    // prediction. The encoder's MeshNormalPrediction operates in the
    // i32 quantized domain, so passing through the integer values
    // directly avoids the precision loss of dequantize → re-quantize.
    // Only ever requested for N=3 Position attributes.
    let ct_indexed: Option<Vec<[i32; 3]>> = if return_ct_indexed && N == 3 {
        let mut out: Vec<[i32; 3]> = vec![[0; 3]; buf_len];
        for (i, v) in partial.iter().enumerate() {
            if visited[i] {
                out[i] = [v[0], v[1], v[2]];
            }
        }
        Some(out)
    } else {
        None
    };

    // Use `from_without_removing_duplicates` — the decoder must
    // preserve the on-wire value ordering so per-vertex lookups via
    // attribute corner table indices stay correct. `Attribute::from`'s
    // dedup pass compacts the buffer and introduces a
    // `point_to_att_val_map` that downstream consumers (decode_to_raw,
    // splice_glb_remove_draco) don't navigate; meshes with duplicate
    // attribute values would otherwise scramble per-vertex lookups.
    let attr = Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    );
    Ok((attr, ct_indexed))
}

/// Read a single attribute component in its original on-wire format
/// (little-endian) and widen it to `f32`. Mirrors Google's base
/// `SequentialAttributeDecoder::DecodeValues`, which copies `byte_stride`
/// raw bytes per value.
fn read_component_as_f32<R: ByteReader>(
    reader: &mut R,
    component_type: ComponentDataType,
) -> Result<f32, Err> {
    Ok(match component_type {
        ComponentDataType::F32 => f32::from_bits(reader.read_u32()?),
        ComponentDataType::F64 => f64::from_bits(reader.read_u64()?) as f32,
        ComponentDataType::U8 => reader.read_u8()? as f32,
        ComponentDataType::I8 => (reader.read_u8()? as i8) as f32,
        ComponentDataType::U16 => reader.read_u16()? as f32,
        ComponentDataType::I16 => (reader.read_u16()? as i16) as f32,
        ComponentDataType::U32 => reader.read_u32()? as f32,
        ComponentDataType::I32 => (reader.read_u32()? as i32) as f32,
        ComponentDataType::U64 => reader.read_u64()? as f32,
        ComponentDataType::I64 => (reader.read_u64()? as i64) as f32,
        ComponentDataType::Invalid => return Err(Err::InvalidComponentDataType),
    })
}

/// Decode flow for the GENERIC sequential encoder (Google's base
/// `SequentialAttributeDecoder`): no prediction scheme, no transform, no
/// quantization, no entropy coding. Values are stored verbatim in their
/// original component format, one per attribute vertex, in mesh-traversal
/// (first-visit) order — exactly the order the quantized path walks. We
/// mirror that traversal so the produced buffer, emitted in vertex-id
/// order, lines up with the corner-table → vertex mapping the rest of the
/// decoder relies on.
fn decode_generic_attribute<R: ByteReader, const N: usize>(
    reader: &mut R,
    meta: &AttributeMeta,
    corner_table: &DecoderCornerTable,
    num_attr_values: usize,
) -> Result<Attribute, Err>
where
    NdVector<N, f32>: Vector<N, Component = f32>,
{
    let sequence = draco_oxide_core::codec::attribute::sequence::compute_sequence_depth_first(corner_table);

    let buf_len = corner_table.num_vertices().max(num_attr_values);
    let mut partial: Vec<[f32; N]> = vec![[0.0; N]; buf_len];
    let mut visited = vec![false; buf_len];

    for c in &sequence {
        let v_idx = usize::from(corner_table.vertex_idx(*c));
        if visited[v_idx] {
            continue;
        }
        let mut value = [0f32; N];
        for comp in value.iter_mut() {
            *comp = read_component_as_f32(reader, meta.component_type)?;
        }
        partial[v_idx] = value;
        visited[v_idx] = true;
    }

    // Emit in vertex-id order (skipping never-visited slots), matching
    // `decode_quantized_attribute`.
    let mut data: Vec<NdVector<N, f32>> = Vec::with_capacity(num_attr_values);
    for (i, v) in partial.iter().enumerate() {
        if !visited[i] {
            continue;
        }
        let mut nd = <NdVector<N, f32> as Vector<N>>::zero();
        for (j, &val) in v.iter().enumerate() {
            *nd.get_mut(j) = val;
        }
        data.push(nd);
    }

    let attr = Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    );
    Ok(attr)
}

/// One parallelogram prediction `P = next + prev - opposite` for corner
/// `c`, computed from the triangle across `c`'s opposite edge. Returns
/// `None` when there's no opposite face or any of the three needed vertices
/// hasn't been decoded yet (Google's `< data_entry_id` guard, which in our
/// vertex-indexed model is the `visited` flag).
///
/// Mirrors `MeshPredictionSchemeParallelogramShared::computeParallelogramPrediction`.
/// (Uses `next(c)`/`prev(c)`; these resolve to the same shared-edge vertices
/// as Google's `next(opp)`/`prev(opp)`, so the value is identical.)
fn compute_parallelogram_pred<CT: draco_oxide_core::corner_table::GenericCornerTable, const N: usize>(
    ct: &CT,
    c: CornerIdx,
    visited: &[bool],
    partial: &[[i32; N]],
) -> Option<[i32; N]> {
    let opp = ct.opposite(c)?;
    let opp_vi = usize::from(ct.vertex_idx(opp));
    let next_vi = usize::from(ct.vertex_idx(ct.next(c)));
    let prev_vi = usize::from(ct.vertex_idx(ct.previous(c)));

    if !visited[opp_vi] || !visited[next_vi] || !visited[prev_vi] {
        return None;
    }

    let a = partial[next_vi];
    let b = partial[prev_vi];
    let diag = partial[opp_vi];

    let mut out = [0i32; N];
    for i in 0..N {
        out[i] = a[i] + b[i] - diag[i];
    }
    Some(out)
}

/// Multi-parallelogram prediction: average every valid parallelogram around
/// the vertex (reached by swinging right from the start corner). Falls back
/// to `last_decoded` (delta from the previous value) when none are valid.
///
/// Mirrors `MeshPredictionSchemeMultiParallelogramDecoder::computeOriginalValues`.
fn compute_multi_parallelogram_pred<CT: draco_oxide_core::corner_table::GenericCornerTable, const N: usize>(
    ct: &CT,
    start: CornerIdx,
    visited: &[bool],
    partial: &[[i32; N]],
    last_decoded: [i32; N],
) -> [i32; N] {
    let mut sum = [0i32; N];
    let mut count = 0i32;

    let mut corner = Some(start);
    while let Some(c) = corner {
        if let Some(par) = compute_parallelogram_pred::<_, N>(ct, c, visited, partial) {
            for k in 0..N {
                sum[k] += par[k];
            }
            count += 1;
        }
        corner = match ct.swing_right(c) {
            Some(next) if next == start => None,
            other => other,
        };
    }

    if count == 0 {
        last_decoded
    } else {
        let mut out = [0i32; N];
        for k in 0..N {
            out[k] = sum[k] / count;
        }
        out
    }
}

/// Constrained-multi-parallelogram prediction. Walks up to
/// `MAX_NUM_PARALLELOGRAMS` parallelograms around the vertex — swinging left
/// from the start corner, then right if a boundary is hit — and averages
/// only those NOT flagged as a crease edge. The crease flags were decoded
/// up front into per-count contexts; `crease_pos` tracks the running cursor
/// per context so flags are consumed in the same order the encoder wrote
/// them. Falls back to `last_decoded` when no parallelogram is used.
///
/// Mirrors `MeshPredictionSchemeConstrainedMultiParallelogramDecoder::computeOriginalValues`.
#[allow(clippy::too_many_arguments)]
fn compute_constrained_multi_parallelogram_pred<CT: draco_oxide_core::corner_table::GenericCornerTable, const N: usize>(
    ct: &CT,
    start: CornerIdx,
    visited: &[bool],
    partial: &[[i32; N]],
    last_decoded: [i32; N],
    crease_edges: &[Vec<bool>; MAX_NUM_PARALLELOGRAMS],
    crease_pos: &mut [usize; MAX_NUM_PARALLELOGRAMS],
) -> Result<[i32; N], Err> {
    // Collect parallelograms in the encoder's walk order (left first, then
    // right from the start corner on boundary).
    let mut preds: [[i32; N]; MAX_NUM_PARALLELOGRAMS] = [[0; N]; MAX_NUM_PARALLELOGRAMS];
    let mut num_parallelograms = 0usize;

    let mut corner = Some(start);
    let mut first_pass = true;
    while let Some(c) = corner {
        if let Some(par) = compute_parallelogram_pred::<_, N>(ct, c, visited, partial) {
            preds[num_parallelograms] = par;
            num_parallelograms += 1;
            if num_parallelograms == MAX_NUM_PARALLELOGRAMS {
                break;
            }
        }

        let mut next = if first_pass {
            ct.swing_left(c)
        } else {
            ct.swing_right(c)
        };
        if next == Some(start) {
            break;
        }
        if next.is_none() && first_pass {
            first_pass = false;
            next = ct.swing_right(start);
        }
        corner = next;
    }

    // Crease flags select which of the found parallelograms contribute.
    let mut multi = [0i32; N];
    let mut num_used = 0i32;
    if num_parallelograms > 0 {
        let context = num_parallelograms - 1;
        for pred in preds.iter().take(num_parallelograms) {
            let pos = crease_pos[context];
            crease_pos[context] += 1;
            let is_crease = *crease_edges[context]
                .get(pos)
                .ok_or(Err::CreaseEdgeFlagUnderrun)?;
            if !is_crease {
                num_used += 1;
                for k in 0..N {
                    multi[k] += pred[k];
                }
            }
        }
    }

    if num_used == 0 {
        Ok(last_decoded)
    } else {
        for m in multi.iter_mut() {
            *m /= num_used;
        }
        Ok(multi)
    }
}

/// Reads the constrained-multi-parallelogram crease-edge flags. One
/// rANS-bit-coded array per parallelogram-count context, each preceded by a
/// varint length. Mirrors
/// `MeshPredictionSchemeConstrainedMultiParallelogramDecoder::decodePredictionData`.
fn read_crease_edge_flags<R: ByteReader>(
    reader: &mut R,
    num_corners: usize,
) -> Result<[Vec<bool>; MAX_NUM_PARALLELOGRAMS], Err> {
    use crate::decode::entropy::rans::RabsDecoder;

    let mut out: [Vec<bool>; MAX_NUM_PARALLELOGRAMS] = Default::default();
    for ctx in out.iter_mut() {
        let num_flags = draco_oxide_core::utils::bit_coder::leb128_read(reader)? as usize;
        if num_flags > num_corners {
            return Err(Err::CreaseEdgeFlagCountInvalid(num_flags));
        }
        if num_flags == 0 {
            continue;
        }
        // RAnsBitDecoder framing: prob-of-zero byte, varint payload size,
        // then the rABS-coded bytes (read back-to-front by RabsDecoder).
        let prob_zero = reader.read_u8()?;
        let buf_len = draco_oxide_core::utils::bit_coder::leb128_read(reader)? as usize;
        let buf = draco_oxide_core::utils::bit_coder::read_byte_buffer(reader, buf_len)?;
        let mut flags = Vec::with_capacity(num_flags);
        if buf_len > 0 {
            let mut iter = buf.into_iter();
            let mut rabs: RabsDecoder<_> =
                RabsDecoder::new(&mut iter, buf_len, prob_zero as usize, None)?;
            for _ in 0..num_flags {
                flags.push(rabs.read().unwrap_or(0) != 0);
            }
        } else {
            flags.resize(num_flags, false);
        }
        *ctx = flags;
    }
    Ok(out)
}

/// Decode flow for normal attributes: N=2 oct-quantized i32 values
/// produced by `MeshNormalPrediction` + `OctahedralOrthogonal` +
/// `OctahedralQuantization`. Output is N=3 unit-vector f32 normals.
///
/// `MeshNormalPrediction` requires positions (the parent attribute)
/// to compute predictions from the corner table; when those aren't
/// available we fall back to `last_decoded` prediction so the byte
/// stream is consumed correctly even though the produced values are
/// only quantized-coherent, not semantically correct.
#[allow(clippy::too_many_arguments)]
fn decode_normal_attribute<R: ByteReader>(
    reader: &mut R,
    meta: &AttributeMeta,
    corner_table: &DecoderCornerTable,
    attr_table: Option<&crate::decode::connectivity::DecoderAttributeCornerTable>,
    num_position_vertices: usize,
    pred_scheme_id: u8,
    _position_parent: Option<&Attribute>,
    positions_by_ct_vertex: Option<&[[i32; 3]]>,
) -> Result<Attribute, Err> {
    use draco_oxide_core::bit_coder::BitReader;
    use draco_oxide_core::buffer::LsbFirst;
    use crate::decode::entropy::rans::RabsDecoder;

    const N: usize = 2;
    // When the encoder uses a per-attribute corner table for normals
    // (normal-seamed mesh), the attribute vertex count differs from
    // the position vertex count.
    let num_attr_values = attr_table
        .map(|t| t.num_vertices)
        .unwrap_or(num_position_vertices);
    let num_symbols = num_attr_values * N;
    let symbols = read_corrections(num_symbols, N, reader)?;

    // Two prediction schemes pair with the octahedral transform:
    //  - GEOMETRIC_NORMAL (6): predict from surrounding triangle geometry,
    //    canonicalize, then a per-vertex flip bit (RABS) chooses the sign.
    //  - DIFFERENCE (0) / NONE: plain delta from the previous octahedral value;
    //    no geometric prediction and NO flip bits.
    let geometric = pred_scheme_id == MESH_GEOMETRIC_NORMAL_PREDICTION_ID;

    let inverse_xform = OctahedralOrthogonalInverseTransform::read(reader)?;

    // MeshNormalPrediction (geometric) writes its own metadata: u8 prob_zero +
    // leb128 len + RABS-coded flip bits (one per normal vertex).
    let mut flips: Vec<bool> = vec![false; num_attr_values];
    if geometric {
        let flip_prob = reader.read_u8()?;
        let flip_buf_len = draco_oxide_core::utils::bit_coder::leb128_read(reader)? as usize;
        let flip_buf = draco_oxide_core::utils::bit_coder::read_byte_buffer(reader, flip_buf_len)?;
        if flip_buf_len > 0 {
            let mut iter = flip_buf.into_iter();
            let mut rabs: RabsDecoder<_> =
                RabsDecoder::new(&mut iter, flip_buf_len, flip_prob as usize, None)?;
            for f in flips.iter_mut() {
                *f = rabs.read().unwrap_or(0) != 0;
            }
        }
    }
    let _ = BitReader::<'_, std::vec::IntoIter<u8>, LsbFirst>::spown_from;

    let dequant = OctahedralNormal::read(reader)?;

    // Helper closures: vertex_idx via the chosen corner table.
    let attr_v_idx = |c: CornerIdx| -> usize {
        match attr_table {
            Some(t) => usize::from(<crate::decode::connectivity::DecoderAttributeCornerTable as draco_oxide_core::corner_table::GenericCornerTable>::vertex_idx(t, c)),
            None => usize::from(corner_table.vertex_idx(c)),
        }
    };
    let universal_v_idx = |c: CornerIdx| -> usize { usize::from(corner_table.vertex_idx(c)) };

    use draco_oxide_core::codec::attribute::sequence::compute_sequence_depth_first;
    let sequence = match attr_table {
        Some(t) => compute_sequence_depth_first(t),
        None => compute_sequence_depth_first(corner_table),
    };

    let buf_len = num_attr_values.max(corner_table.num_vertices());
    let mut partial: Vec<[i32; 2]> = vec![[0; 2]; buf_len];
    let mut visited = vec![false; buf_len];
    let mut symbol_idx = 0usize;
    let mut flip_idx = 0usize;
    // Delta-prediction running value (octahedral domain). Google's
    // PredictionSchemeDeltaDecoder seeds the first prediction with a zero
    // vector, NOT the octahedral center.
    let mut last_decoded: [i32; 2] = [0; 2];

    for c in &sequence {
        let v_idx = attr_v_idx(*c);
        if visited[v_idx] {
            continue;
        }

        let pred = if geometric {
            let p = match positions_by_ct_vertex {
                Some(positions) => predict_normal(
                    corner_table,
                    attr_table,
                    *c,
                    positions,
                    inverse_xform.center_value,
                    inverse_xform.max_quantized_value,
                    flips.get(flip_idx).copied().unwrap_or(false),
                    universal_v_idx(*c),
                ),
                None => [inverse_xform.center_value, inverse_xform.center_value],
            };
            flip_idx += 1;
            p
        } else {
            // DIFFERENCE / NONE: delta from the previously decoded value.
            last_decoded
        };

        if symbol_idx + N > symbols.len() {
            return Err(Err::SymbolStreamUnderrun);
        }
        let corr = [symbols[symbol_idx] as i32, symbols[symbol_idx + 1] as i32];
        symbol_idx += N;

        let value = inverse_xform.inverse(&corr, &pred);
        partial[v_idx] = value;
        last_decoded = value;
        visited[v_idx] = true;
    }

    if symbol_idx != symbols.len() {
        return Err(Err::SymbolStreamSurplus);
    }

    // Dequantize 2D oct → 3D unit normal.
    let mut data: Vec<NdVector<3, f32>> = Vec::with_capacity(num_attr_values);
    for (i, v) in partial.iter().enumerate() {
        if !visited[i] {
            continue;
        }
        let n = dequant.dequantize(v);
        let mut nd = <NdVector<3, f32> as Vector<3>>::zero();
        *nd.get_mut(0) = n[0];
        *nd.get_mut(1) = n[1];
        *nd.get_mut(2) = n[2];
        data.push(nd);
    }

    // Use `from_without_removing_duplicates` — the decoder must
    // preserve the on-wire value ordering so per-vertex lookups via
    // attribute corner table indices stay correct. `Attribute::from`'s
    // dedup pass compacts the buffer and introduces a
    // `point_to_att_val_map` that downstream consumers (decode_to_raw,
    // splice_glb_remove_draco) don't navigate; meshes with duplicate
    // attribute values would otherwise scramble per-vertex lookups.
    let attr = Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    );
    Ok(attr)
}

/// Inverse `MeshNormalPrediction`. Mirrors
/// `shared/attribute/prediction_scheme/mesh_normal_prediction.rs::predict`:
///   1. Sum face normals around vertex of corner `c` (cross products of
///      neighbour-position deltas).
///   2. Cast down + apply `octahedral_transform` to project onto the
///      octahedron face.
///   3. Scale to oct-quantized i32.
///   4. If the encoder's flip bit is set, negate the result.
fn predict_normal(
    ct: &DecoderCornerTable,
    attr_table: Option<&crate::decode::connectivity::DecoderAttributeCornerTable>,
    c: CornerIdx,
    positions_by_ct_vertex: &[[i32; 3]],
    center_value: i32,
    max_quantized_value: i32,
    flip: bool,
    universal_v_idx: usize,
) -> [i32; 2] {
    use draco_oxide_core::corner_table::GenericCornerTable;
    let pos_c = positions_by_ct_vertex
        .get(universal_v_idx)
        .copied()
        .unwrap_or([0; 3]);

    // Walk to leftmost adjacent corner. When attr_table is set, use
    // its swing (which respects seam edges as boundaries) so we sum
    // face normals only over the FAN that belongs to this attribute
    // vertex. Otherwise, walk the full universal 1-ring.
    let swing_left = |curr: CornerIdx| -> Option<CornerIdx> {
        match attr_table {
            Some(t) => GenericCornerTable::swing_left(t, curr),
            None => ct.swing_left(curr),
        }
    };
    let swing_right = |curr: CornerIdx| -> Option<CornerIdx> {
        match attr_table {
            Some(t) => GenericCornerTable::swing_right(t, curr),
            None => ct.swing_right(curr),
        }
    };

    // Mirror Google's VertexCornersIterator: emit corner_id first,
    // walk SwingLeft until boundary, then SwingRight from start_corner
    // until another boundary. Avoids the "swing-left-to-leftmost,
    // then swing-right-from-there" indirection — semantically the
    // same SET of corners but with explicit boundary handling.
    let mut sum: [i64; 3] = face_normal_i64(ct, c, pos_c, positions_by_ct_vertex);
    {
        let mut curr = c;
        loop {
            match swing_left(curr) {
                Some(next) if next == c => break,
                Some(next) => {
                    curr = next;
                    let f = face_normal_i64(ct, curr, pos_c, positions_by_ct_vertex);
                    for k in 0..3 {
                        sum[k] += f[k];
                    }
                }
                None => {
                    // Boundary — switch to swinging right from `c`.
                    let mut r = c;
                    while let Some(rn) = swing_right(r) {
                        r = rn;
                        let f = face_normal_i64(ct, r, pos_c, positions_by_ct_vertex);
                        for k in 0..3 {
                            sum[k] += f[k];
                        }
                    }
                    break;
                }
            }
        }
    }

    // Cap |sum| ≤ 2^29 to keep i32 conversions safe (mirrors Google's
    // GeometricNormalPredictorArea::ComputePredictedValue cap).
    let upper_bound: i64 = 1 << 29;
    let abs = sum[0].abs() + sum[1].abs() + sum[2].abs();
    if abs > upper_bound {
        let q = abs / upper_bound;
        if q > 0 {
            for s in sum.iter_mut() {
                *s /= q;
            }
        }
    }

    let mut vec3 = [sum[0] as i32, sum[1] as i32, sum[2] as i32];

    // CanonicalizeIntegerVector: project onto the octahedron surface
    // such that |v[0]| + |v[1]| + |v[2]| == center_value. Mirrors
    // Google's OctahedronToolBox::CanonicalizeIntegerVector.
    let abs_sum = (vec3[0].abs() as i64) + (vec3[1].abs() as i64) + (vec3[2].abs() as i64);
    if abs_sum == 0 {
        vec3[0] = center_value;
    } else {
        vec3[0] = (((vec3[0] as i64) * (center_value as i64)) / abs_sum) as i32;
        vec3[1] = (((vec3[1] as i64) * (center_value as i64)) / abs_sum) as i32;
        if vec3[2] >= 0 {
            vec3[2] = center_value - vec3[0].abs() - vec3[1].abs();
        } else {
            vec3[2] = -(center_value - vec3[0].abs() - vec3[1].abs());
        }
    }

    // Flip in 3D BEFORE oct conversion (Google does
    // `pred_normal_3d = -pred_normal_3d` on the canonicalized 3D
    // normal, then converts to oct).
    if flip {
        vec3[0] = -vec3[0];
        vec3[1] = -vec3[1];
        vec3[2] = -vec3[2];
    }

    // IntegerVectorToQuantizedOctahedralCoords + CanonicalizeOctahedralCoords.
    integer_vector_to_quantized_oct(vec3, center_value, max_quantized_value)
}

/// Mirror of Google's
/// `OctahedronToolBox::IntegerVectorToQuantizedOctahedralCoords` +
/// `CanonicalizeOctahedralCoords` (`normal_compression_utils.h`).
fn integer_vector_to_quantized_oct(
    int_vec: [i32; 3],
    center_value: i32,
    max_value: i32,
) -> [i32; 2] {
    let mut s;
    let mut t;
    if int_vec[0] >= 0 {
        // Right hemisphere.
        s = int_vec[1] + center_value;
        t = int_vec[2] + center_value;
    } else {
        // Left hemisphere.
        s = if int_vec[1] < 0 {
            int_vec[2].abs()
        } else {
            max_value - int_vec[2].abs()
        };
        t = if int_vec[2] < 0 {
            int_vec[1].abs()
        } else {
            max_value - int_vec[1].abs()
        };
    }
    // CanonicalizeOctahedralCoords: snap edge points to canonical positions.
    if (s == 0 && (t == 0 || t == max_value)) || (s == max_value && t == 0) {
        s = max_value;
        t = max_value;
    } else if s == 0 && t > center_value {
        t = center_value - (t - center_value);
    } else if s == max_value && t < center_value {
        t = center_value + (center_value - t);
    } else if t == max_value && s < center_value {
        s = center_value + (center_value - s);
    } else if t == 0 && s > center_value {
        s = center_value - (s - center_value);
    }
    [s, t]
}

fn face_normal_i64(
    ct: &DecoderCornerTable,
    c: CornerIdx,
    pos_c: [i32; 3],
    positions_by_ct_vertex: &[[i32; 3]],
) -> [i64; 3] {
    let next_vi = usize::from(ct.vertex_idx(ct.next(c)));
    let prev_vi = usize::from(ct.vertex_idx(ct.previous(c)));
    let pn = positions_by_ct_vertex
        .get(next_vi)
        .copied()
        .unwrap_or([0; 3]);
    let pp = positions_by_ct_vertex
        .get(prev_vi)
        .copied()
        .unwrap_or([0; 3]);
    let dn = [pn[0] - pos_c[0], pn[1] - pos_c[1], pn[2] - pos_c[2]];
    let dp = [pp[0] - pos_c[0], pp[1] - pos_c[1], pp[2] - pos_c[2]];
    [
        (dn[1] as i64) * (dp[2] as i64) - (dn[2] as i64) * (dp[1] as i64),
        (dn[2] as i64) * (dp[0] as i64) - (dn[0] as i64) * (dp[2] as i64),
        (dn[0] as i64) * (dp[1] as i64) - (dn[1] as i64) * (dp[0] as i64),
    ]
}

/// Mirrors `encode/attribute/prediction_transform/geom.rs::into_faithful_oct_quantization`.
#[allow(dead_code)] // Float-path oct quantization helper, kept for reference vs the int port.
fn into_faithful_oct_quantization(vec: [i32; 2], max: i32) -> [i32; 2] {
    let half = max / 2;
    let u = vec[0];
    let v = vec[1];
    let mut x = u;
    let mut y = v;
    if (u == max || u == 0) && v == 0 || (u == 0 && v == max) {
        return [max, max];
    } else if u == 0 && v > half {
        y = half - (v - half);
    } else if u == max && v < half {
        y = half + (half - v);
    } else if v == max && u < half {
        x = half + (half - u);
    } else if v == 0 && u > half {
        x = half - (u - half);
    }
    [x, y]
}

/// Decode flow for TextureCoordinate attributes that use
/// `MeshPredictionForTextureCoordinates` + `WrappedDifference` +
/// `QuantizationCoordinateWise`. Mirrors
/// `shared/attribute/prediction_scheme/mesh_prediction_for_texture_coordinates.rs`.
///
/// Byte layout this consumes (after the 3 header bytes already read):
/// 1. RANS-coded UV symbols (2 per visited vertex).
/// 2. Prediction metadata (this scheme):
///    - u32 — orientation bit count (one bit per complex prediction).
///    - u8 — RABS zero_prob.
///    - leb128 — RABS buffer length.
///    - bytes — RABS-coded RLE bits (encoder reverses them then runs
///      `o == last_orientation ? 1 : 0`).
/// 3. WrappedDifference transform metadata (min, max).
/// 4. QuantizationCoordinateWise deportabilization metadata.
fn decode_uv_attribute<R: ByteReader>(
    reader: &mut R,
    meta: &AttributeMeta,
    corner_table: &DecoderCornerTable,
    attr_table: Option<&crate::decode::connectivity::DecoderAttributeCornerTable>,
    _start_corners: &[CornerIdx],
    num_position_vertices: usize,
    xform_kind: InverseTransformKind,
    positions_by_ct_vertex: Option<&[[i32; 3]]>,
) -> Result<Attribute, Err> {
    use crate::decode::entropy::rans::RabsDecoder;

    const N: usize = 2;
    // The symbol count = number of *attribute* vertices times N. When
    // attr_table is present, that's the seam-split UV vertex count; else
    // fall back to the position vertex count (no UV seams).
    let num_attr_values = attr_table
        .map(|t| t.num_vertices)
        .unwrap_or(num_position_vertices);
    let num_symbols = num_attr_values * N;
    let symbols = read_corrections(num_symbols, N, reader)?;

    let orientation_count = {
        let b0 = reader.read_u8()?;
        let b1 = reader.read_u8()?;
        let b2 = reader.read_u8()?;
        let b3 = reader.read_u8()?;
        u32::from_le_bytes([b0, b1, b2, b3]) as usize
    };
    let flip_prob = reader.read_u8()?;
    let rabs_buf_len = draco_oxide_core::utils::bit_coder::leb128_read(reader)? as usize;
    let rabs_buf = draco_oxide_core::utils::bit_coder::read_byte_buffer(reader, rabs_buf_len)?;

    let inverse_xform = InverseTransform::read(reader, xform_kind)?;
    let dequant = Quantization::read(reader, N)?;

    // Mirror Google's RAnsBitDecoder semantics. Google's encoder calls
    // EncodeBit in forward order over orientations[0..N-1], its
    // EndEncoding reverses bits before rabs_write so RABS's LIFO read
    // brings them back to forward order in the decoder. Decoder reads
    // bits in EncodeBit order, applies forward delta-RLE to populate
    // orientations[0..N-1]. (See Google's
    // mesh_prediction_scheme_tex_coords_portable_decoder.h::DecodePredictionData
    // and rans_bit_encoder.cc::EndEncoding.)
    let mut bits = Vec::with_capacity(orientation_count);
    if rabs_buf_len > 0 && orientation_count > 0 {
        let mut iter = rabs_buf.into_iter();
        let mut rabs: RabsDecoder<_> =
            RabsDecoder::new(&mut iter, rabs_buf_len, flip_prob as usize, None)?;
        for _ in 0..orientation_count {
            bits.push(rabs.read().unwrap_or(0) != 0);
        }
    }
    let mut last = true;
    let mut orientations = Vec::with_capacity(orientation_count);
    for &b in &bits {
        if !b {
            last = !last;
        }
        orientations.push(last);
    }

    // Helper: pick the right corner-table vertex_idx for storage. When
    // attr_table is set, `v_idx` indexes attribute slots (post-seam-
    // split). Otherwise universal vertex IDs.
    let attr_v_idx = |c: CornerIdx| -> usize {
        match attr_table {
            Some(t) => usize::from(<crate::decode::connectivity::DecoderAttributeCornerTable as draco_oxide_core::corner_table::GenericCornerTable>::vertex_idx(t, c)),
            None => usize::from(corner_table.vertex_idx(c)),
        }
    };
    let universal_v_idx = |c: CornerIdx| -> usize { usize::from(corner_table.vertex_idx(c)) };

    // Set up traversal seeds. When attr_table is set, use the
    // start_corners (per-component start corners from edgebreaker
    // start-face replay) — these match what the encoder uses
    // (`corners_of_edgebreaker`). When attr_table is None, use face
    // seeds (one per face) since we're walking the universal corner
    // table and need to reach all faces.
    use draco_oxide_core::codec::attribute::sequence::compute_sequence_depth_first;
    let sequence = match attr_table {
        Some(t) => compute_sequence_depth_first(t),
        None => compute_sequence_depth_first(corner_table),
    };

    let buf_len = num_attr_values.max(corner_table.num_vertices());
    let mut partial: Vec<[i32; 2]> = vec![[0; 2]; buf_len];
    let mut visited = vec![false; buf_len];
    let mut symbol_idx = 0usize;
    // Google's decoder pops orientations from the BACK during the
    // forward iteration of corners. This is because the encoder
    // iterates p = N-1..0 (reverse), so the i-th complex-prediction
    // call in DECODER's forward iteration corresponds to the
    // i-th-FROM-END encoder push.
    let mut orientations_remaining = orientations;
    let mut last_decoded: [i32; 2] = [0; 2];

    for c in &sequence {
        let v_idx = attr_v_idx(*c);
        if visited[v_idx] {
            continue;
        }

        let next_c = corner_table.next(*c);
        let prev_c = corner_table.previous(*c);
        let next_vi = attr_v_idx(next_c);
        let prev_vi = attr_v_idx(prev_c);

        let pred = if visited[next_vi] && visited[prev_vi] {
            // Encoder: when both neighbors visited AND UVs equal,
            // returns prev_uv directly (no orientation push). Mirror
            // that — falling through to fallback would give a different
            // value AND mess up the orientation index.
            if partial[next_vi] == partial[prev_vi] {
                partial[prev_vi]
            } else {
                // Complex prediction path. Encoder pushes one orientation
                // here. Compute both candidate UVs and pick. Position
                // lookups use UNIVERSAL vertex IDs (not attribute).
                let pred_pair = uv_predict_complex(
                    partial[v_idx],
                    partial[next_vi],
                    partial[prev_vi],
                    positions_by_ct_vertex,
                    universal_v_idx(*c),
                    universal_v_idx(next_c),
                    universal_v_idx(prev_c),
                );
                match pred_pair {
                    Some((p0, p1)) => {
                        let orient = orientations_remaining.pop().unwrap_or(true);
                        if orient {
                            p0
                        } else {
                            p1
                        }
                    }
                    // Encoder hit overflow guard → fallback (no orientation
                    // pushed). Decoder must also fall back.
                    None => uv_predict_fallback_attr(*c, next_vi, &visited, &partial, last_decoded),
                }
            }
        } else {
            uv_predict_fallback_attr(*c, next_vi, &visited, &partial, last_decoded)
        };

        if symbol_idx + N > symbols.len() {
            return Err(Err::SymbolStreamUnderrun);
        }
        let mut corr = [0i32; N];
        for i in 0..N {
            corr[i] = symbols[symbol_idx + i] as i32;
        }
        symbol_idx += N;

        let mut value = [0i32; N];
        inverse_xform.inverse_n(&corr, &pred, &mut value);
        partial[v_idx] = value;
        last_decoded = value;
        visited[v_idx] = true;
    }

    if symbol_idx != symbols.len() {
        return Err(Err::SymbolStreamSurplus);
    }

    // Dequantize to f32 in attribute-vertex-id-ascending order,
    // straight into the final `Vec<NdVector<N, f32>>`.
    let mut data: Vec<NdVector<N, f32>> = Vec::with_capacity(num_attr_values);
    let mut tmp = vec![0f32; N];
    for (i, v) in partial.iter().enumerate() {
        if !visited[i] {
            continue;
        }
        dequant.dequantize_into(v, &mut tmp);
        let mut nd = <NdVector<N, f32> as Vector<N>>::zero();
        for (j, &val) in tmp.iter().enumerate() {
            *nd.get_mut(j) = val;
        }
        data.push(nd);
    }
    // Use `from_without_removing_duplicates` — the decoder must
    // preserve the on-wire value ordering so per-vertex lookups via
    // attribute corner table indices stay correct. `Attribute::from`'s
    // dedup pass compacts the buffer and introduces a
    // `point_to_att_val_map` that downstream consumers (decode_to_raw,
    // splice_glb_remove_draco) don't navigate; meshes with duplicate
    // attribute values would otherwise scramble per-vertex lookups.
    let attr = Attribute::from_without_removing_duplicates(
        AttributeId::new(meta.unique_id as usize),
        data,
        meta.attribute_type,
        meta.domain,
        Vec::new(),
    );
    Ok(attr)
}

/// Inverse of `MeshPredictionForTextureCoordinates::predict` complex path.
/// Returns `Some((predicted_uv_0, predicted_uv_1))` matching the two
/// orientation choices the encoder considers, or `None` if any of the
/// encoder-side overflow guards trips (in which case the encoder
/// fell back without pushing an orientation).
fn uv_predict_complex(
    _curr_uv_unused: [i32; 2],
    next_uv_i32: [i32; 2],
    prev_uv_i32: [i32; 2],
    positions_by_ct_vertex: Option<&[[i32; 3]]>,
    curr_vi: usize,
    next_vi: usize,
    prev_vi: usize,
) -> Option<([i32; 2], [i32; 2])> {
    let positions = positions_by_ct_vertex?;
    let curr_pos = positions.get(curr_vi).copied()?;
    let next_pos = positions.get(next_vi).copied()?;
    let prev_pos = positions.get(prev_vi).copied()?;
    let curr_pos = [curr_pos[0] as i64, curr_pos[1] as i64, curr_pos[2] as i64];
    let next_pos = [next_pos[0] as i64, next_pos[1] as i64, next_pos[2] as i64];
    let prev_pos = [prev_pos[0] as i64, prev_pos[1] as i64, prev_pos[2] as i64];
    let next_uv = [next_uv_i32[0] as i64, next_uv_i32[1] as i64];
    let prev_uv = [prev_uv_i32[0] as i64, prev_uv_i32[1] as i64];

    let pn = [
        prev_pos[0] - next_pos[0],
        prev_pos[1] - next_pos[1],
        prev_pos[2] - next_pos[2],
    ];
    let pn_norm2_squared = (pn[0] * pn[0] + pn[1] * pn[1] + pn[2] * pn[2]) as u64;
    if pn_norm2_squared == 0 {
        return None;
    }
    let cn = [
        curr_pos[0] - next_pos[0],
        curr_pos[1] - next_pos[1],
        curr_pos[2] - next_pos[2],
    ];
    let cn_dot_pn = pn[0] * cn[0] + pn[1] * cn[1] + pn[2] * cn[2];
    let pn_uv = [prev_uv[0] - next_uv[0], prev_uv[1] - next_uv[1]];

    // Match encoder overflow guards.
    let n_uv_absmax = next_uv[0].abs().max(next_uv[1].abs());
    if pn_norm2_squared as i64 != 0 && n_uv_absmax > i64::MAX / pn_norm2_squared as i64 {
        return None;
    }
    let pn_uv_absmax = pn_uv[0].abs().max(pn_uv[1].abs());
    if pn_uv_absmax != 0 && cn_dot_pn.abs() > i64::MAX / pn_uv_absmax {
        return None;
    }

    let x_uv = [
        next_uv[0] * pn_norm2_squared as i64 + pn_uv[0] * cn_dot_pn,
        next_uv[1] * pn_norm2_squared as i64 + pn_uv[1] * cn_dot_pn,
    ];

    let pn_absmax = pn[0].abs().max(pn[1].abs()).max(pn[2].abs());
    if pn_absmax != 0 && cn_dot_pn.abs() > i64::MAX / pn_absmax {
        return None;
    }
    let pn_norm2_i = pn_norm2_squared as i64;
    // Encoder: `next_pos + pn * cn_dot_pn / pn_norm2_squared` which
    // evaluates as element-wise `(pn[k] * cn_dot_pn) / pn_norm2`.
    let x_pos = [
        next_pos[0] + (pn[0] * cn_dot_pn) / pn_norm2_i,
        next_pos[1] + (pn[1] * cn_dot_pn) / pn_norm2_i,
        next_pos[2] + (pn[2] * cn_dot_pn) / pn_norm2_i,
    ];
    let cx = [
        curr_pos[0] - x_pos[0],
        curr_pos[1] - x_pos[1],
        curr_pos[2] - x_pos[2],
    ];
    let cx_norm2_squared = (cx[0] * cx[0] + cx[1] * cx[1] + cx[2] * cx[2]) as u64;

    let mut cx_uv = [pn_uv[1], -pn_uv[0]];
    let prod = cx_norm2_squared.checked_mul(pn_norm2_squared)?;
    let norm_squared = prod.isqrt() as i64;
    cx_uv[0] *= norm_squared;
    cx_uv[1] *= norm_squared;

    let p0 = [
        ((x_uv[0] + cx_uv[0]) / pn_norm2_i) as i32,
        ((x_uv[1] + cx_uv[1]) / pn_norm2_i) as i32,
    ];
    let p1 = [
        ((x_uv[0] - cx_uv[0]) / pn_norm2_i) as i32,
        ((x_uv[1] - cx_uv[1]) / pn_norm2_i) as i32,
    ];
    Some((p0, p1))
}

/// Mirrors `MeshPredictionForTextureCoordinates::fallback_predict`.
/// Variant that takes pre-resolved attribute vertex indices so the
/// caller can use either the universal or attribute corner table for
/// vertex lookup.
fn uv_predict_fallback_attr(
    _c: CornerIdx,
    next_vi: usize,
    visited: &[bool],
    partial: &[[i32; 2]],
    last_decoded: [i32; 2],
) -> [i32; 2] {
    if next_vi < visited.len() && visited[next_vi] {
        return partial[next_vi];
    }
    last_decoded
}

/// On-wire prediction-scheme method IDs. These match Google's
/// `PredictionSchemeMethod` enum (compression_shared.h): the byte the
/// integer attribute decoder reads right after the encoder-type/transform
/// header.
///   0    → PREDICTION_DIFFERENCE (delta from previous value)
///   1    → MESH_PREDICTION_PARALLELOGRAM (single parallelogram)
///   2    → MESH_PREDICTION_MULTI_PARALLELOGRAM (averaged parallelograms)
///   4    → MESH_PREDICTION_CONSTRAINED_MULTI_PARALLELOGRAM (crease-selected)
///   5    → MESH_PREDICTION_TEX_COORDS_PORTABLE (UV path, handled elsewhere)
///   6    → MESH_PREDICTION_GEOMETRIC_NORMAL (normal path, handled elsewhere)
///   0xFE → PREDICTION_NONE
const DELTA_PREDICTION_ID: u8 = 0;
const MESH_PARALLELOGRAM_PREDICTION_ID: u8 = 1;
const MESH_MULTI_PARALLELOGRAM_PREDICTION_ID: u8 = 2;
const MESH_CONSTRAINED_MULTI_PARALLELOGRAM_PREDICTION_ID: u8 = 4;
const MESH_PREDICTION_FOR_TEXTURE_COORDINATES_ID: u8 = 5;
const MESH_GEOMETRIC_NORMAL_PREDICTION_ID: u8 = 6;
const NO_PREDICTION_ID: u8 = 0xFE;

/// Constrained-multi-parallelogram caps the number of parallelograms
/// averaged at one vertex; crease-edge flags are grouped into this many
/// per-count contexts. Matches Google's `kMaxNumParallelograms`.
const MAX_NUM_PARALLELOGRAMS: usize = 4;

/// The position-prediction schemes our quantized/integer decode path
/// supports, resolved from the on-wire method id. Mirrors the subset of
/// Google's `PredictionSchemeDecoderFactory` that applies to corner-table
/// meshes with the WRAP transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionPredictionScheme {
    /// Delta from the previously decoded value (also covers PREDICTION_NONE).
    Delta,
    Parallelogram,
    MultiParallelogram,
    ConstrainedMultiParallelogram,
}

impl PositionPredictionScheme {
    fn from_id(id: u8) -> Result<Self, Err> {
        match id {
            DELTA_PREDICTION_ID | NO_PREDICTION_ID => Ok(Self::Delta),
            MESH_PARALLELOGRAM_PREDICTION_ID => Ok(Self::Parallelogram),
            MESH_MULTI_PARALLELOGRAM_PREDICTION_ID => Ok(Self::MultiParallelogram),
            MESH_CONSTRAINED_MULTI_PARALLELOGRAM_PREDICTION_ID => {
                Ok(Self::ConstrainedMultiParallelogram)
            }
            other => Err(Err::PredictionSchemeTodo(other)),
        }
    }
}

/// Reads only the per-attribute metadata block (steps 1-3 of the byte
/// layout). Useful for diagnostics / smoke tests before the full decode
/// pipeline lands.
pub(crate) fn read_metadata<R: ByteReader>(
    reader: &mut R,
    header: &Header,
) -> Result<Vec<AttributeMeta>, Err> {
    let num_attrs = reader.read_u8()? as usize;

    let mut decoder_ids: Vec<Option<u8>> = vec![None; num_attrs];
    let mut domains: Vec<AttributeDomain> = Vec::with_capacity(num_attrs);
    let mut traversals: Vec<TraversalType> = Vec::with_capacity(num_attrs);

    if header.encoding_method == EncoderMethod::Edgebreaker {
        for slot in decoder_ids.iter_mut().take(num_attrs) {
            *slot = Some(reader.read_u8()?);
            domains
                .push(AttributeDomain::read_from(reader).map_err(|_| Err::InvalidAttributeDomain)?);
            traversals.push(TraversalType::from_id(reader.read_u8()?)?);
        }
    } else {
        // Sequential: encoder writes nothing here. Defaults are fine.
        for _ in 0..num_attrs {
            domains.push(AttributeDomain::Position);
            traversals.push(TraversalType::DepthFirst);
        }
    }

    let mut metas = Vec::with_capacity(num_attrs);
    for i in 0..num_attrs {
        let _count = reader.read_u8()?; // always 1 in current encoder
        let attribute_type =
            AttributeType::read_from(reader).map_err(|_| Err::InvalidAttributeType)?;
        let component_type =
            ComponentDataType::read_from(reader).map_err(|_| Err::InvalidComponentDataType)?;
        let num_components = reader.read_u8()?;
        let normalized = reader.read_u8()?;
        let unique_id = reader.read_u8()?;
        let portabilization = PortabilizationType::from_id(reader.read_u8()?)?;

        metas.push(AttributeMeta {
            decoder_id: decoder_ids[i],
            domain: domains[i],
            traversal: traversals[i],
            attribute_type,
            component_type,
            num_components,
            normalized,
            unique_id,
            portabilization,
        });
    }

    Ok(metas)
}
