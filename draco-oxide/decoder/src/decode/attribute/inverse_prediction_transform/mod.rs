//! Inverse of `encode/attribute/prediction_transform/`.
//!
//! Given a stream of decoded "positive i32" symbols + a per-attribute
//! prediction value, produces the original quantized i32 attribute value.
//! The inverse transform is the second-to-last decode step:
//!
//!   symbols → from_positive_i32 → pred + corr (with wrap) → quantized i32
//!                                                              ↓
//!                                                       deportabilize

use draco_oxide_core::bit_coder::ReaderErr;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Reader error: {0}")]
    Reader(#[from] ReaderErr),
    #[error("Invalid prediction transform id: {0}")]
    InvalidId(u8),
    #[error("OctahedralReflection / Orthogonal inverse transforms not yet implemented")]
    OctahedralTodo,
}

/// Mirrors `encode/attribute/prediction_transform/mod.rs::PredictionTransformType::get_id`:
///   0xFF → NoTransform
///   0    → Difference
///   1    → WrappedDifference
///   2    → OctahedralReflection
///   3    → OctahedralOrthogonal
///   4    → Orthogonal
#[derive(Debug, Clone, Copy)]
pub(crate) enum InverseTransformKind {
    NoTransform,
    Difference,
    WrappedDifference,
    OctahedralReflection,
    OctahedralOrthogonal,
    Orthogonal,
}

impl InverseTransformKind {
    pub(crate) fn from_id(id: u8) -> Result<Self, Err> {
        match id {
            0xFF => Ok(Self::NoTransform),
            0 => Ok(Self::Difference),
            1 => Ok(Self::WrappedDifference),
            2 => Ok(Self::OctahedralReflection),
            3 => Ok(Self::OctahedralOrthogonal),
            4 => Ok(Self::Orthogonal),
            _ => Err(Err::InvalidId(id)),
        }
    }
}

/// N-component inverse prediction transform. Used by:
/// - Position (N=3): WrappedDifference + 11-bit quantization.
/// - TextureCoordinate (N=2): WrappedDifference + 10-bit quantization.
/// - Custom (N=*): WrappedDifference + ToBits.
/// - Normal (N=2): handled by `OctahedralOrthogonalInverseTransform` below.
pub(crate) enum InverseTransform {
    NoTransform,
    Difference,
    WrappedDifference { min: i32, max: i32, max_diff: i32 },
}

impl InverseTransform {
    pub(crate) fn read<R: crate::prelude::ByteReader>(
        reader: &mut R,
        kind: InverseTransformKind,
    ) -> Result<Self, Err> {
        match kind {
            InverseTransformKind::NoTransform => Ok(Self::NoTransform),
            InverseTransformKind::Difference => Ok(Self::Difference),
            InverseTransformKind::WrappedDifference => {
                let min = read_i32(reader)?;
                let max = read_i32(reader)?;
                let max_diff = 1 + (max - min);
                Ok(Self::WrappedDifference { min, max, max_diff })
            }
            InverseTransformKind::OctahedralOrthogonal
            | InverseTransformKind::OctahedralReflection
            | InverseTransformKind::Orthogonal => Err(Err::OctahedralTodo),
        }
    }

    /// Applies the inverse transform per-component:
    /// `(corr_positive[N], pred[N]) → orig[N]`. `corr_positive` is the
    /// symbol value as decoded from the stream (still in zigzag form).
    pub(crate) fn inverse_n(&self, corr_positive: &[i32], pred: &[i32], out: &mut [i32]) {
        debug_assert_eq!(corr_positive.len(), pred.len());
        debug_assert_eq!(corr_positive.len(), out.len());

        match self {
            Self::NoTransform => {
                for i in 0..corr_positive.len() {
                    out[i] = from_positive_i32(corr_positive[i]);
                }
            }
            Self::Difference => {
                for i in 0..corr_positive.len() {
                    out[i] = from_positive_i32(corr_positive[i]) + pred[i];
                }
            }
            Self::WrappedDifference { min, max, max_diff } => {
                for i in 0..corr_positive.len() {
                    let corr = from_positive_i32(corr_positive[i]);
                    let pred_clamped = pred[i].clamp(*min, *max);
                    let mut val = pred_clamped + corr;
                    if val > *max {
                        val -= *max_diff;
                    } else if val < *min {
                        val += *max_diff;
                    }
                    out[i] = val;
                }
            }
        }
    }
}

/// `OctahedralOrthogonal` inverse for normals (always 2-component).
/// Mirrors `encode/attribute/prediction_transform/oct_orthogonal.rs`. Our
/// encoder hardcodes max=255 + center=127 (8-bit oct grid); Google may
/// emit a different `max_quantized_value` per the per-attribute
/// quantization bits, so we read both u32s and use them.
pub(crate) struct OctahedralOrthogonalInverseTransform {
    pub max_quantized_value: i32,
    pub center_value: i32,
}

impl OctahedralOrthogonalInverseTransform {
    pub(crate) fn read<R: crate::prelude::ByteReader>(reader: &mut R) -> Result<Self, Err> {
        let max_quantized_value = read_u32(reader)? as i32;
        let center_value = read_u32(reader)? as i32;
        Ok(Self {
            max_quantized_value,
            center_value,
        })
    }

    /// Exact inverse of Google's `NORMAL_OCTAHEDRON_CANONICALIZED` transform
    /// (`PredictionSchemeNormalOctahedronCanonicalizedDecodingTransform::
    /// ComputeOriginalValue`). `pred` and `corr` are 2-component i32 arrays;
    /// `corr` is the raw (non-zigzag) correction. Bit-exactness matters: this
    /// runs once per normal AND feeds itself under delta prediction, so any
    /// per-step drift accumulates across the whole attribute.
    pub(crate) fn inverse(&self, corr: &[i32; 2], pred: &[i32; 2]) -> [i32; 2] {
        let center = self.center_value;
        let max = self.max_quantized_value;

        let mut pred_s = pred[0] - center;
        let mut pred_t = pred[1] - center;

        let pred_in_diamond = pred_s.abs() + pred_t.abs() <= center;
        if !pred_in_diamond {
            let (s, t) = invert_diamond(pred_s, pred_t, center);
            pred_s = s;
            pred_t = t;
        }

        let pred_in_bottom_left =
            (pred_s == 0 && pred_t == 0) || (pred_s < 0 && pred_t <= 0);
        let rot = rotation_count(pred_s, pred_t);
        if !pred_in_bottom_left {
            let (s, t) = rotate_point(pred_s, pred_t, rot);
            pred_s = s;
            pred_t = t;
        }

        let mut orig_s = mod_max(pred_s + corr[0], center, max);
        let mut orig_t = mod_max(pred_t + corr[1], center, max);

        if !pred_in_bottom_left {
            let (s, t) = rotate_point(orig_s, orig_t, (4 - rot) & 3);
            orig_s = s;
            orig_t = t;
        }
        if !pred_in_diamond {
            let (s, t) = invert_diamond(orig_s, orig_t, center);
            orig_s = s;
            orig_t = t;
        }

        [orig_s + center, orig_t + center]
    }
}

/// Folds a point outside the octahedral diamond back inside (and its inverse —
/// the transform is its own inverse). Mirrors the `invert_diamond` block in
/// Google's canonicalized octahedron transform.
fn invert_diamond(s: i32, t: i32, center: i32) -> (i32, i32) {
    let (sign_s, sign_t) = if s >= 0 && t >= 0 {
        (1, 1)
    } else if s <= 0 && t <= 0 {
        (-1, -1)
    } else {
        (if s > 0 { 1 } else { -1 }, if t > 0 { 1 } else { -1 })
    };
    let corner_s = sign_s * center;
    let corner_t = sign_t * center;
    let mut us = s * 2 - corner_s;
    let mut ut = t * 2 - corner_t;
    if sign_s * sign_t >= 0 {
        let tmp = us;
        us = -ut;
        ut = -tmp;
    } else {
        std::mem::swap(&mut us, &mut ut);
    }
    ((us + corner_s) / 2, (ut + corner_t) / 2)
}

/// Quadrant-based rotation count used to canonicalize the prediction into the
/// bottom-left octant before applying the correction.
fn rotation_count(s: i32, t: i32) -> u32 {
    if s == 0 {
        if t > 0 {
            3
        } else if t < 0 {
            1
        } else {
            0
        }
    } else if s > 0 {
        if t >= 0 {
            2
        } else {
            1
        }
    } else if t > 0 {
        3
    } else {
        0
    }
}

/// Rotates `(s, t)` by `count` quarter-turns (matches the encoder/decoder's
/// fixed-count rotation, not a generic CCW loop).
fn rotate_point(s: i32, t: i32, count: u32) -> (i32, i32) {
    match count {
        1 => (t, -s),
        2 => (-s, -t),
        3 => (-t, s),
        _ => (s, t),
    }
}

/// Mirrors Google's `OctahedronToolBox::ModMax`:
///   if x > center: return x - max
///   if x < -center: return x + max
///   else: return x
#[inline]
fn mod_max(x: i32, center: i32, max: i32) -> i32 {
    if x > center {
        x - max
    } else if x < -center {
        x + max
    } else {
        x
    }
}

/// Inverse of `utils::to_positive_i32`:
///   0 → 0,  1 → -1,  2 → 1,  3 → -2,  4 → 2, …
#[inline]
pub(crate) fn from_positive_i32(p: i32) -> i32 {
    if p & 1 == 0 {
        p >> 1
    } else {
        -((p >> 1) + 1)
    }
}

fn read_i32<R: crate::prelude::ByteReader>(reader: &mut R) -> Result<i32, ReaderErr> {
    let bytes = [
        reader.read_u8()?,
        reader.read_u8()?,
        reader.read_u8()?,
        reader.read_u8()?,
    ];
    Ok(i32::from_le_bytes(bytes))
}

fn read_u32<R: crate::prelude::ByteReader>(reader: &mut R) -> Result<u32, ReaderErr> {
    let bytes = [
        reader.read_u8()?,
        reader.read_u8()?,
        reader.read_u8()?,
        reader.read_u8()?,
    ];
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local copy of `utils::to_positive_i32` (which is `pub(crate)` in
    /// another module) — duplicating to avoid coupling test to private API.
    fn to_positive_i32(val: i32) -> i32 {
        if val >= 0 {
            val << 1
        } else {
            (-(val + 1) << 1) + 1
        }
    }

    #[test]
    fn from_positive_i32_round_trips() {
        for v in -50..=50i32 {
            let p = to_positive_i32(v);
            assert_eq!(from_positive_i32(p), v, "round-trip failed for {}", v);
        }
    }

    #[test]
    fn from_positive_i32_known_values() {
        assert_eq!(from_positive_i32(0), 0);
        assert_eq!(from_positive_i32(1), -1);
        assert_eq!(from_positive_i32(2), 1);
        assert_eq!(from_positive_i32(3), -2);
        assert_eq!(from_positive_i32(4), 2);
        assert_eq!(from_positive_i32(99), -50);
        assert_eq!(from_positive_i32(100), 50);
    }
}
