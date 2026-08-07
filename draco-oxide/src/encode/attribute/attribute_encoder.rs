use std::vec;

use crate::encode::entropy::symbol_coding::encode_vector_symbols;
use draco_oxide_core::attribute::Attribute;
use draco_oxide_core::attribute::AttributeType;
use draco_oxide_core::attribute::{AttributeDomain, ComponentDataType};
use draco_oxide_core::bit_coder::ByteWriter;
use draco_oxide_core::codec::attribute::sequence::Traverser;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::mesh::ds::{AttributeDS, GenericAttributeDs};
use draco_oxide_core::types::ConfigType;
use draco_oxide_core::types::{CornerIdx, DataValue, NdVector, PointIdx};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Err {
    #[error("Entropy Symbol Encoding Error: {0}")]
    EntropyEncodingError(#[from] crate::encode::entropy::symbol_coding::Err),
    #[error("Invalid attribute id: {0}")]
    InvalidAttributeId(usize),
    #[error("Invalid prediction scheme id: {0}")]
    InvalidPredictionSchemeId(usize),
    #[error("Attribute Encoder has too many encoding groups: {0}")]
    TooManyEncodingGroups(usize),
    #[error("An attribute has too many parents: {0}")]
    TooManyParents(usize),
    #[error("64-bit integer components have no compressed representation; the reference carries them only through the raw generic codec, which is not implemented")]
    Unsupported64BitComponents,
    #[error("Unsupported data type.")]
    UnsupportedDataType,
    #[error("Attribute data has too many components; it must be less than {}, but it is {}.", 5, .0)]
    // ToDo: Change 5 to the build config
    UnsupportedNumComponents(usize),
    #[error("Prediction Error: {0}")]
    PredictionError(#[from] super::prediction_metadata::Err),
}

#[derive(Clone, Debug)]
pub struct GroupConfig {
    pub prediction_scheme: prediction_scheme::Config,
    pub prediction_transform: prediction_transform::Config,
}

impl GroupConfig {
    #[allow(clippy::single_range_in_vec_init, clippy::needless_update)]
    fn default_for(att_ty: AttributeType, component_ty: ComponentDataType) -> Self {
        // Integer values ride the integer codec with wrap-transformed
        // predictions whatever the attribute type; the geometric pipelines
        // below assume float input (float quantization, octahedral normals).
        if component_ty.is_integer() {
            let prediction = match att_ty {
                AttributeType::Position | AttributeType::TextureCoordinate => {
                    prediction_scheme::PredictionSchemeType::MeshParallelogramPrediction
                }
                _ => prediction_scheme::PredictionSchemeType::DeltaPrediction,
            };
            return Self {
                prediction_scheme: prediction_scheme::Config { ty: prediction },
                prediction_transform: prediction_transform::Config {
                    ty: prediction_transform::PredictionTransformType::WrappedDifference,
                    portabilization: portabilization::Config::default_for(att_ty, component_ty),
                },
            };
        }
        match att_ty {
            AttributeType::Position => Self {
                prediction_scheme: prediction_scheme::Config {
                    ty: prediction_scheme::PredictionSchemeType::MeshParallelogramPrediction,
                    ..prediction_scheme::Config::default()
                },
                prediction_transform: prediction_transform::Config {
                    ty: prediction_transform::PredictionTransformType::WrappedDifference,
                    portabilization: portabilization::Config::default_for(att_ty, component_ty),
                },
            },
            AttributeType::Normal => Self {
                prediction_scheme: prediction_scheme::Config {
                    ty: prediction_scheme::PredictionSchemeType::MeshNormalPrediction,
                    ..prediction_scheme::Config::default()
                },
                prediction_transform: prediction_transform::Config {
                    ty: prediction_transform::PredictionTransformType::OctahedralOrthogonal,
                    portabilization: portabilization::Config::default_for(att_ty, component_ty),
                },
            },
            // Parallelogram over the UV connectivity is the default: it
            // decodes substantially faster than the geometric texture scheme
            // (no position reads, no orientation bits, no integer sqrt) at a
            // small ratio cost on heavily distorted atlases. The geometric
            // scheme stays available as a per-attribute override.
            AttributeType::TextureCoordinate => Self {
                prediction_scheme: prediction_scheme::Config {
                    ty: prediction_scheme::PredictionSchemeType::MeshParallelogramPrediction,
                    ..prediction_scheme::Config::default()
                },
                prediction_transform: prediction_transform::Config {
                    ty: prediction_transform::PredictionTransformType::WrappedDifference,
                    portabilization: portabilization::Config::default_for(att_ty, component_ty),
                },
            },
            // Color (e.g. glTF COLOR_0), a generic per-vertex attribute with no
            // mesh-geometry predictor. The reference Draco decoder
            // (`SequentialIntegerAttributeDecoder::CreateIntPredictionScheme`)
            // builds a prediction scheme ONLY when the transform type is
            // `PREDICTION_TRANSFORM_WRAP` (id 1); any other transform, including
            // the `Difference`/`PREDICTION_TRANSFORM_DELTA` (id 0) default the
            // `_` catch-all selects, makes the decoder skip the prediction
            // revert and return the raw quantized residuals (garbage colors,
            // alpha read as delta-of-constant). Pin Color to the
            // reference-compatible delta + wrapped-difference path
            // (PREDICTION_DIFFERENCE + WRAP, also the reference encoder's
            // generic high-speed path), so draco3d reconstructs absolute colors.
            AttributeType::Color => Self {
                prediction_scheme: prediction_scheme::Config {
                    ty: prediction_scheme::PredictionSchemeType::DeltaPrediction,
                    ..prediction_scheme::Config::default()
                },
                prediction_transform: prediction_transform::Config {
                    ty: prediction_transform::PredictionTransformType::WrappedDifference,
                    portabilization: portabilization::Config::default_for(
                        AttributeType::Color,
                        component_ty,
                    ),
                },
            },
            // Any other type (tangents, weights, unrecognized generics) takes
            // the same reference-compatible delta + wrapped-difference pipeline
            // as the Color arm: the reference decoder reverts predictions only
            // under the wrap transform, so the plain `Difference` default would
            // decode to raw residuals there.
            _ => Self {
                prediction_scheme: prediction_scheme::Config {
                    ty: prediction_scheme::PredictionSchemeType::DeltaPrediction,
                    ..prediction_scheme::Config::default()
                },
                prediction_transform: prediction_transform::Config {
                    ty: prediction_transform::PredictionTransformType::WrappedDifference,
                    portabilization: portabilization::Config::default_for(att_ty, component_ty),
                },
            },
        }
    }
}

/// How the correction stream for an attribute is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingMode {
    /// Predict every value, transform against the prediction, and encode the real
    /// corrections (the standard, lossy-by-quantization path).
    Full,
    /// Emit an all-zero correction stream and neutral prediction metadata, so the
    /// decoder reconstructs exactly what it predicts. The input attribute's values
    /// are never read, and only its seams matter. Currently only meaningful for
    /// normals under
    /// [`MeshNormalPrediction`](draco_oxide_core::codec::attribute::prediction_scheme::mesh_normal_prediction),
    /// where it makes normal compression effectively zero-CPU.
    ZeroCorrection,
}

#[derive(Clone, Debug)]
pub struct Config {
    group_cfgs: Vec<GroupConfig>,
    rans_encoding: bool,
    mode: EncodingMode,
}

// ToDo: THIS IMPLEMENTATION IS NOT FINAL
impl ConfigType for Config {
    fn default() -> Self {
        Self {
            group_cfgs: Vec::new(),
            rans_encoding: true,
            mode: EncodingMode::Full,
        }
    }
}

impl Config {
    pub fn default_for(att_ty: AttributeType, component_ty: ComponentDataType) -> Self {
        Self {
            group_cfgs: vec![GroupConfig::default_for(att_ty, component_ty)],
            rans_encoding: true,
            mode: EncodingMode::Full,
        }
    }

    /// Zero-CPU normal encoding: keeps the normal prediction/transform metadata of
    /// the default normal path, but the encoder synthesizes an all-zero correction
    /// stream instead of reading the input normals (see [`EncodingMode::ZeroCorrection`]).
    pub fn predicted_normals() -> Self {
        Self {
            group_cfgs: vec![GroupConfig::default_for(
                AttributeType::Normal,
                ComponentDataType::F32,
            )],
            rans_encoding: true,
            mode: EncodingMode::ZeroCorrection,
        }
    }

    /// Overrides the prediction scheme of the (single) encoding group.
    pub fn set_prediction_scheme(&mut self, ty: prediction_scheme::PredictionSchemeType) {
        self.group_cfgs[0].prediction_scheme.ty = ty;
    }

    /// Overrides the prediction transform of the (single) encoding group.
    pub fn set_prediction_transform(&mut self, ty: prediction_transform::PredictionTransformType) {
        self.group_cfgs[0].prediction_transform.ty = ty;
    }

    /// Restricts the configuration to what a linearly sequenced stream can
    /// express. Such a stream carries no connectivity, so every mesh-geometry
    /// scheme degrades to delta prediction; the prediction transforms are
    /// unaffected.
    pub(super) fn for_sequential(mut self) -> Self {
        use prediction_scheme::PredictionSchemeType as S;
        for group in &mut self.group_cfgs {
            if group.prediction_scheme.ty != S::NoPrediction {
                group.prediction_scheme.ty = S::DeltaPrediction;
            }
        }
        self
    }

    /// Overrides the quantization resolution of the (single) encoding group.
    pub fn set_quantization(&mut self, quantization: portabilization::Quantization) {
        self.group_cfgs[0]
            .prediction_transform
            .portabilization
            .quantization = quantization;
    }
}

/// The order an attribute's values are encoded in.
#[derive(Clone, Copy, Debug)]
pub(super) enum Sequencing {
    /// A traversal of the attribute's own connectivity, seeded by the
    /// edgebreaker traversal. One value per attribute vertex.
    Traversal,
    /// The point space in index order. One value per point, so a value split
    /// across a seam is stored once for each point it belongs to.
    Linear { num_points: usize },
}

/// Where this attribute's traversal sequence comes from. Attributes without
/// interior seams share the position connectivity, so their walks are
/// identical per traversal method: the first such attribute walks and records
/// the sequence, and later ones replay the recording borrowed. An attribute
/// with its own connectivity walks lazily and never materializes the full
/// sequence.
pub(super) enum SequenceSource<'s> {
    /// Replay the sequence recorded by an earlier attribute's walk.
    Shared(&'s [CornerIdx]),
    /// Walk this attribute's connectivity into the given buffer for later
    /// attributes to replay.
    Record(&'s mut Vec<CornerIdx>),
    /// Drive this attribute's lazy walk without recording.
    Own,
}

pub(super) struct AttributeEncoder<'parents, 'encoder, 'writer, 'ds, W> {
    cfg: Config,
    writer: &'writer mut W,
    parents: &'encoder [&'parents Attribute],
    ads: AttributeDS<'ds>,
    sequencing: Sequencing,
    /// Corners of the edgebreaker traversal, used to seed this attribute's sequencing.
    corners_of_edgebreaker: &'encoder [CornerIdx],
    /// This attribute's traversal sequence source.
    sequence: SequenceSource<'encoder>,
}

impl<'parents, 'encoder, 'writer, 'ds, W> AttributeEncoder<'parents, 'encoder, 'writer, 'ds, W>
where
    W: ByteWriter,
    'parents: 'encoder,
{
    pub(super) fn new(
        ads: AttributeDS<'ds>,
        parents: &'encoder [&'parents Attribute],
        corners_of_edgebreaker: &'encoder [CornerIdx],
        writer: &'writer mut W,
        cfg: Config,
        sequencing: Sequencing,
        sequence: SequenceSource<'encoder>,
    ) -> Self {
        AttributeEncoder {
            cfg,
            writer,
            parents,
            ads,
            sequencing,
            corners_of_edgebreaker,
            sequence,
        }
    }

    /// Writes this attribute's payload block and returns its portable
    /// representation together with its portabilization metadata. The metadata
    /// is returned rather than written because an encoder carrying several
    /// attributes emits every payload before the first metadata block.
    pub(super) fn encode<const WRITE_NOW: bool>(mut self) -> Result<(Attribute, Vec<u8>), Err> {
        if matches!(
            self.ads.att_data().get_component_type(),
            ComponentDataType::I64 | ComponentDataType::U64
        ) {
            return Err(Err::Unsupported64BitComponents);
        }
        self.cfg.group_cfgs[0]
            .prediction_scheme
            .ty
            .write_to(self.writer);
        // The reference frames PREDICTION_NONE without a transform: no
        // transform id byte and no transform data, the values ride the symbol
        // coder untransformed.
        if self.cfg.group_cfgs[0].prediction_scheme.ty
            == prediction_scheme::PredictionSchemeType::NoPrediction
        {
            self.cfg.group_cfgs[0].prediction_transform.ty =
                prediction_transform::PredictionTransformType::NoTransform;
        } else {
            self.cfg.group_cfgs[0]
                .prediction_transform
                .ty
                .write_to(self.writer);
        }

        if self.cfg.mode == EncodingMode::ZeroCorrection {
            return self.encode_zero_correction_normal();
        }

        let component_type = self.ads.att_data().get_component_type();
        match component_type {
            ComponentDataType::F32 => self.unpack_num_components::<WRITE_NOW, f32>(),
            ComponentDataType::F64 => self.unpack_num_components::<WRITE_NOW, f64>(),
            ComponentDataType::U8 => self.unpack_num_components::<WRITE_NOW, u8>(),
            ComponentDataType::U16 => self.unpack_num_components::<WRITE_NOW, u16>(),
            ComponentDataType::U32 => self.unpack_num_components::<WRITE_NOW, u32>(),
            ComponentDataType::U64 => self.unpack_num_components::<WRITE_NOW, u64>(),
            ComponentDataType::I8 => self.unpack_num_components::<WRITE_NOW, i8>(),
            ComponentDataType::I16 => self.unpack_num_components::<WRITE_NOW, i16>(),
            ComponentDataType::I32 => self.unpack_num_components::<WRITE_NOW, i32>(),
            ComponentDataType::I64 => self.unpack_num_components::<WRITE_NOW, i64>(),
            ComponentDataType::Invalid => Err(Err::UnsupportedDataType),
        }
    }

    /// Emits the zero-CPU normal stream: an all-zero octahedral correction
    /// sequence plus the same transform/prediction/portabilization metadata the
    /// default normal path writes, so Google Draco (and our decoder) rebuild the
    /// geometry-derived predicted normals. The input normal values are never read;
    /// only the connectivity-derived value count (the traversal length) is used.
    fn encode_zero_correction_normal(mut self) -> Result<(Attribute, Vec<u8>), Err> {
        let num_values = match std::mem::replace(&mut self.sequence, SequenceSource::Own) {
            SequenceSource::Shared(s) => s.len(),
            SequenceSource::Record(buf) => {
                buf.extend(Traverser::new(
                    &self.ads,
                    self.corners_of_edgebreaker.to_vec(),
                ));
                buf.len()
            }
            SequenceSource::Own => {
                Traverser::new(&self.ads, self.corners_of_edgebreaker.to_vec()).count()
            }
        };

        const N: usize = 2;

        let por_cfg =
            portabilization::Config::default_for(AttributeType::Normal, ComponentDataType::F32);
        let mut port_info_buffer: Vec<u8> = Vec::new();
        port_info_buffer.write_u8(por_cfg.quantization.resolve(0.0));

        let mut transform_info_buffer: Vec<u8> = Vec::new();
        let transform = PredictionTransform::<N>::new(self.cfg.group_cfgs[0].prediction_transform);
        let _ = transform.squeeze(&mut transform_info_buffer);

        self.writer.write_u8(self.cfg.rans_encoding as u8);
        if self.cfg.rans_encoding {
            let zeros = vec![NdVector::<N, i32>::zero(); num_values];
            encode_vector_symbols(&zeros, self.writer)?;
        } else {
            let zero = NdVector::<N, i32>::zero();
            for _ in 0..num_values {
                zero.write_to(self.writer);
            }
        }

        for byte in transform_info_buffer {
            self.writer.write_u8(byte);
        }
        super::prediction_metadata::encode_flip_metadata(&vec![false; num_values], self.writer)?;

        // Normals are a leaf attribute, so this returned octahedral attribute is
        // never consulted as a parent; an empty 2-component attribute suffices.
        Ok((
            Attribute::from_without_removing_duplicates::<NdVector<N, i32>, N>(
                self.ads.att_data().get_id(),
                Vec::new(),
                AttributeType::Normal,
                self.ads.att_data().get_domain(),
                self.ads.att_data().get_parents().clone(),
            ),
            port_info_buffer,
        ))
    }

    fn unpack_num_components<const WRITE_NOW: bool, T>(self) -> Result<(Attribute, Vec<u8>), Err>
    where
        T: DataValue + Copy,
        NdVector<1, T>: Vector<1>,
        NdVector<2, T>: Vector<2>,
        NdVector<3, T>: Vector<3>,
        NdVector<4, T>: Vector<4>,
    {
        let num_components = self.ads.att_data().get_num_components();
        match num_components {
            0 => unreachable!("Vector of dimension 0 is not allowed"),
            1 => self.encode_typed::<WRITE_NOW, 1, _>(),
            2 => self.encode_typed::<WRITE_NOW, 2, _>(),
            3 => self.encode_typed::<WRITE_NOW, 3, _>(),
            4 => self.encode_typed::<WRITE_NOW, 4, _>(),
            _ => Err(Err::UnsupportedNumComponents(num_components)),
        }
    }

    fn encode_typed<const WRITE_NOW: bool, const N: usize, T>(
        self,
    ) -> Result<(Attribute, Vec<u8>), Err>
    where
        T: DataValue + Copy,
        NdVector<N, T>: Vector<N> + Portable,
        NdVector<N, i32>: Vector<N, Component = i32>,
        NdVector<N, f32>: Vector<N, Component = f32> + Portable,
    {
        self.encode_impl::<WRITE_NOW, NdVector<N, T>, N>()
    }

    fn encode_impl<const WRITE_NOW: bool, Data, const N: usize>(
        mut self,
    ) -> Result<(Attribute, Vec<u8>), Err>
    where
        Data: Vector<N> + Portable,
        NdVector<N, i32>: Vector<N, Component = i32>,
        NdVector<N, f32>: Vector<N, Component = f32> + Portable,
    {
        // Use the (possibly overridden) portabilization config from the encoding
        // group; `GroupConfig::default_for` seeds it with the per-type default, so
        // an unconfigured attribute reproduces `default_for(ty)`.
        let por_cfg = self.cfg.group_cfgs[0].prediction_transform.portabilization;

        let mut att = Attribute::new(
            Vec::<Data>::new(),
            AttributeType::Position,
            AttributeDomain::Position,
            Vec::new(),
        );
        std::mem::swap(&mut att, self.ads.att_data_mut());
        let mut port_info_buffer = Vec::new();
        let portabilization: portabilization::Portabilization<Data, N> =
            portabilization::Portabilization::new(att, por_cfg, &mut port_info_buffer);
        let port_att = portabilization.portabilize();

        match port_att.get_num_components() {
            1 => self.encode_portabilized::<1>(port_att, port_info_buffer),
            2 => self.encode_portabilized::<2>(port_att, port_info_buffer),
            3 => self.encode_portabilized::<3>(port_att, port_info_buffer),
            4 => self.encode_portabilized::<4>(port_att, port_info_buffer),
            _ => Err(Err::UnsupportedNumComponents(port_att.get_num_components())),
        }
    }

    fn encode_portabilized<const N: usize>(
        &mut self,
        port_att: Attribute,
        port_info_buffer: Vec<u8>,
    ) -> Result<(Attribute, Vec<u8>), Err>
    where
        NdVector<N, i32>: Vector<N, Component = i32> + Portable,
    {
        // Taken before the prediction scheme borrows `self.ads`.
        let sequence = std::mem::replace(&mut self.sequence, SequenceSource::Own);

        let mut prediction_scheme = prediction_scheme::PredictionScheme::new(
            self.cfg.group_cfgs[0].prediction_scheme.ty.clone(),
            self.parents,
            &self.ads,
            self.cfg.group_cfgs[0]
                .prediction_transform
                .portabilization
                .oct_center(),
        );

        // Transform the predicted values
        let mut transform = PredictionTransform::new(self.cfg.group_cfgs[0].prediction_transform);

        // Predict and transform the values.
        match self.sequencing {
            Sequencing::Traversal => {
                prediction_scheme.dispatch_mut(TraversalRun {
                    ads: &self.ads,
                    port_att: &port_att,
                    transform: &mut transform,
                    sequence,
                    corners_of_edgebreaker: self.corners_of_edgebreaker,
                });
            }
            // A linear sequence carries no connectivity, so the only prediction
            // available is the preceding value, taken as zero at the first
            // point. `Config::for_sequential` pins the scheme to match.
            Sequencing::Linear { num_points } => {
                let mut previous = NdVector::<N, i32>::zero();
                for p in 0..num_points {
                    let val: NdVector<N, i32> = port_att.get(PointIdx::from(p));
                    transform.map_with_tentative_metadata(val, previous);
                    previous = val;
                }
            }
        }

        // Write the output
        let mut transform_info_buffer = Vec::new();
        let mut output = transform.squeeze(&mut transform_info_buffer);

        // Without a prediction scheme nothing guarantees positive values, so
        // the reference codec zigzag-converts them; NoPrediction must match
        // (every other configured transform emits positive corrections).
        if prediction_scheme.get_type() == prediction_scheme::PredictionSchemeType::NoPrediction {
            for v in &mut output {
                for i in 0..N {
                    let x = *v.get(i);
                    *v.get_mut(i) = if x >= 0 {
                        x << 1
                    } else {
                        ((-(x + 1)) << 1) | 1
                    };
                }
            }
        }

        self.writer.write_u8(self.cfg.rans_encoding as u8);
        if self.cfg.rans_encoding {
            encode_vector_symbols(&output, self.writer)?;
        } else {
            // If RANS encoding is not used, we write the output directly
            for value in output {
                value.write_to(self.writer);
            }
        }

        // We need to write the metadata for the prediction, prediction scheme, and transform.
        // This part is a bit tricky, as we need to swap the order of transform and prediction metadata
        // depending on the prediction type, in order to be compatible with the draco decoder.
        if prediction_scheme.get_type()
            == prediction_scheme::PredictionSchemeType::MeshNormalPrediction
        {
            for byte in transform_info_buffer {
                self.writer.write_u8(byte);
            }
            prediction_scheme.encode_prediction_metadata(self.writer)?;
        } else if matches!(
            prediction_scheme.get_type(),
            prediction_scheme::PredictionSchemeType::MeshPredictionForTextureCoordinates
                | prediction_scheme::PredictionSchemeType::MeshConstrainedMultiParallelogramPrediction
        ) {
            prediction_scheme.encode_prediction_metadata(self.writer)?;
            for byte in transform_info_buffer {
                self.writer.write_u8(byte);
            }
        } else {
            // otherwise, the prediction scheme does not have metadata
            assert!({
                let mut buffer = Vec::new();
                prediction_scheme.encode_prediction_metadata(&mut buffer)?;
                buffer.is_empty()
            });
            for byte in transform_info_buffer {
                self.writer.write_u8(byte);
            }
        }

        Ok((port_att, port_info_buffer))
    }
}

/// The traversal-sequenced predict-and-transform loop, monomorphic over the
/// prediction scheme.
struct TraversalRun<'a, 's, 'ds, const N: usize> {
    ads: &'a AttributeDS<'ds>,
    port_att: &'a Attribute,
    transform: &'a mut PredictionTransform<N>,
    sequence: SequenceSource<'s>,
    corners_of_edgebreaker: &'a [CornerIdx],
}

impl<'parents, 'a, 's, 'ds, const N: usize>
    prediction_scheme::SchemeDispatch<'parents, N, AttributeDS<'ds>>
    for TraversalRun<'a, 's, 'ds, N>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    type Out = ();

    fn run<P: prediction_scheme::PredictionSchemeImpl<'parents, N, AttributeDS<'ds>>>(
        self,
        scheme: &mut P,
    ) {
        let TraversalRun {
            ads,
            port_att,
            transform,
            sequence,
            corners_of_edgebreaker,
        } = self;
        let mut sequence_record = Vec::new();
        let mut step = |c: CornerIdx| {
            let val = scheme.predict::<true>(c, &sequence_record, port_att);
            let v = ads.vertex_idx(c);
            sequence_record.push(v);
            let p = ads.global_ds().point_idx(c);
            transform.map_with_tentative_metadata(port_att.get(p), val);
        };
        match sequence {
            SequenceSource::Shared(s) => s.iter().copied().for_each(step),
            // Recording materializes the sequence regardless, and the
            // walk runs measurably faster unfused, so fill first and
            // replay.
            SequenceSource::Record(buf) => {
                *buf = Traverser::new(ads, corners_of_edgebreaker.to_vec()).compute_seqeunce();
                buf.iter().copied().for_each(step);
            }
            // The walk and the prediction loop thrash each other's
            // cache when interleaved per corner, so the lazy walk fills
            // a bounded chunk that is then replayed in batch.
            SequenceSource::Own => {
                let chunk_len = 1024.min(ads.vertex_index_bound());
                let mut walker = Traverser::new(ads, corners_of_edgebreaker.to_vec());
                let mut chunk = Vec::with_capacity(chunk_len);
                loop {
                    chunk.clear();
                    chunk.extend(walker.by_ref().take(chunk_len));
                    if chunk.is_empty() {
                        break;
                    }
                    chunk.iter().copied().for_each(&mut step);
                }
            }
        }
    }
}

use super::prediction_metadata::PredictionEncoder;
use super::prediction_transform::{self, PredictionTransform};
use crate::encode::attribute::portabilization;
use crate::encode::attribute::prediction_transform::PredictionTransformImpl;
use draco_oxide_core::codec::attribute::prediction_scheme;
use draco_oxide_core::types::Vector;
