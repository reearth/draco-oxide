//! Inverse prediction transforms: difference, wrap (min/max), and octahedral
//! orthogonal. The octahedral reflection transform has no encoder-side
//! implementation yet and is rejected as unimplemented.

use crate::Err;
use draco_oxide_core::bit_coder::Reader;
use draco_oxide_core::codec::attribute::geom::invert_diamond;
use draco_oxide_core::codec::attribute::Portable;
use draco_oxide_core::types::{NdVector, Vector};

/// The prediction transform ids on the wire, as written by the encoder.
pub(crate) const TRANSFORM_NONE: u8 = 0xFF;
pub(crate) const TRANSFORM_DIFFERENCE: u8 = 0;
pub(crate) const TRANSFORM_WRAPPED_DIFFERENCE: u8 = 1;
pub(crate) const TRANSFORM_OCT_REFLECTION: u8 = 2;
pub(crate) const TRANSFORM_OCT_ORTHOGONAL: u8 = 3;

/// A parsed prediction transform, ready to reconstruct original values from
/// (prediction, correction) pairs.
pub(crate) enum InverseTransform {
    /// No transform: the stored value is the original value.
    None,
    /// Plain difference; corrections are zigzagged.
    Difference,
    /// Difference wrapped into the value range `[min, max]`; corrections are
    /// zigzagged.
    Wrapped { min: i32, max: i32 },
    /// Octahedral orthogonal transform; corrections are raw (non-negative).
    OctahedralOrthogonal { center: i32, max_quantized: i32 },
}

impl InverseTransform {
    /// Parses the transform metadata for `transform_id` from `reader` (the
    /// `squeeze` output of the encoder-side transform).
    pub(crate) fn read_from(reader: &mut Reader<'_>, transform_id: u8) -> Result<Self, Err> {
        match transform_id {
            TRANSFORM_NONE => Ok(InverseTransform::None),
            TRANSFORM_DIFFERENCE => Ok(InverseTransform::Difference),
            TRANSFORM_WRAPPED_DIFFERENCE => {
                let min = i32::read_from(reader)?;
                let max = i32::read_from(reader)?;
                Ok(InverseTransform::Wrapped { min, max })
            }
            TRANSFORM_OCT_ORTHOGONAL => {
                let max_quantized = reader.read_u32()? as i32;
                let center = reader.read_u32()? as i32;
                Ok(InverseTransform::OctahedralOrthogonal {
                    center,
                    max_quantized,
                })
            }
            TRANSFORM_OCT_REFLECTION => Err(Err::Unimplemented),
            _ => Err(Err::MalformedAttribute("unknown prediction transform id")),
        }
    }

    /// Whether the encoded corrections are zigzag-mapped and need `unzigzag`.
    pub(crate) fn corrections_are_zigzagged(&self) -> bool {
        matches!(
            self,
            InverseTransform::Difference | InverseTransform::Wrapped { .. }
        )
    }

    /// Reconstructs the original value from the prediction and the (already
    /// unzigzagged where applicable) correction.
    #[inline]
    pub(crate) fn compute_original<const N: usize>(
        &self,
        pred: NdVector<N, i32>,
        corr: NdVector<N, i32>,
    ) -> NdVector<N, i32>
    where
        NdVector<N, i32>: Vector<N, Component = i32>,
    {
        match *self {
            InverseTransform::None => corr,
            InverseTransform::Difference => pred + corr,
            InverseTransform::Wrapped { min, max } => {
                let max_diff = 1 + max - min;
                let mut out = NdVector::<N, i32>::zero();
                for i in 0..N {
                    // The encoder clamped the prediction into [min, max] before
                    // differencing.
                    let p = *pred.get(i).clamp(&min, &max);
                    let mut val = p + *corr.get(i);
                    if val > max {
                        val -= max_diff;
                    } else if val < min {
                        val += max_diff;
                    }
                    *out.get_mut(i) = val;
                }
                out
            }
            InverseTransform::OctahedralOrthogonal {
                center,
                max_quantized,
            } => oct_orthogonal_original(pred, corr, center, max_quantized),
        }
    }
}

/// Replays the encoder's canonicalization of the prediction (diamond inversion
/// plus clockwise rotations into the bottom-left quadrant), adds the correction
/// modulo the octahedron edge, and undoes the canonicalization.
fn oct_orthogonal_original<const N: usize>(
    pred_in: NdVector<N, i32>,
    corr: NdVector<N, i32>,
    center: i32,
    max_quantized: i32,
) -> NdVector<N, i32>
where
    NdVector<N, i32>: Vector<N, Component = i32>,
{
    assert!(N == 2);
    let one = center;

    let mut pred = NdVector::<2, i32>::from([*pred_in.get(0) - one, *pred_in.get(1) - one]);

    let inverted = pred.get(0).abs() + pred.get(1).abs() > one;
    if inverted {
        invert_diamond(&mut pred, one);
    }

    let mut rotations = 0usize;
    if pred != NdVector::<2, i32>::zero() {
        while *pred.get(0) >= 0 || *pred.get(1) > 0 {
            let tmp = *pred.get(0);
            *pred.get_mut(0) = -*pred.get(1);
            *pred.get_mut(1) = tmp;
            rotations += 1;
        }
    }

    let mut orig = NdVector::<2, i32>::zero();
    for i in 0..2 {
        let mut val = *pred.get(i) + *corr.get(i);
        if val > one {
            val -= max_quantized;
        } else if val < -one {
            val += max_quantized;
        }
        *orig.get_mut(i) = val;
    }

    // Undo the clockwise rotations with counter-clockwise ones.
    for _ in 0..rotations {
        let tmp = *orig.get(0);
        *orig.get_mut(0) = *orig.get(1);
        *orig.get_mut(1) = -tmp;
    }

    if inverted {
        invert_diamond(&mut orig, one);
    }

    let mut out = NdVector::<N, i32>::zero();
    *out.get_mut(0) = *orig.get(0) + one;
    *out.get_mut(1) = *orig.get(1) + one;
    out
}
