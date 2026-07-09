//! Per-attribute deportabilization (inverse of `encode/attribute/portabilization`).
//!
//! Given a stream of decoded i32 values + portabilization metadata bytes,
//! produces the original-format attribute values.
//!
//! Three flavours covering the encoder side:
//! - `Quantization { N }` — inverse of `QuantizationCoordinateWise`.
//!   Outputs N-component f32 in the original mesh's coordinate range.
//!   Used by Position (N=3), TextureCoordinate (N=2).
//! - `PassThroughI32` — inverse of `ToBits`. Used by Custom attributes.
//! - `OctahedralNormal` — inverse of `OctahedralQuantization` (2D oct
//!   quantized i32 → 3D unit normal f32). Used by Normal.

pub(crate) mod dequantization_coordinate_wise;
pub(crate) mod to_bits;

use draco_oxide_core::bit_coder::ReaderErr;
use crate::prelude::ByteReader;

#[derive(Debug, thiserror::Error)]
pub enum Err {
    #[error("Reader error: {0}")]
    Reader(#[from] ReaderErr),
    #[error("Invalid portabilization id (encoder writes 1=ToBits, 2=Quant, 3=Octahedral): {0}")]
    InvalidId(u8),
    #[error("Octahedral deportabilization requires N=2 input but got N={0}")]
    OctahedralWrongInputN(usize),
    #[error("Octahedral quantization bits {0} out of supported range (1..=31)")]
    OctahedralBitsOutOfRange(u8),
}

/// On-wire IDs (encoder side: `PortabilizationType::get_id`):
///   1 → ToBits / Integer
///   2 → QuantizationCoordinateWise
///   3 → OctahedralQuantization
#[derive(Debug, Clone, Copy)]
pub(crate) enum DeportabilizationKind {
    ToBits,
    QuantizationCoordinateWise,
    OctahedralQuantization,
}

impl DeportabilizationKind {
    #[allow(dead_code)] // Reserved for the public-API portabilization-by-id dispatch.
    pub(crate) fn from_id(id: u8) -> Result<Self, Err> {
        match id {
            1 => Ok(Self::ToBits),
            2 => Ok(Self::QuantizationCoordinateWise),
            3 => Ok(Self::OctahedralQuantization),
            _ => Err(Err::InvalidId(id)),
        }
    }
}

/// Quantization parameters for an N-component coordinate-wise quantization
/// (positions, UVs). Output is f32 of the same dimensionality as the input.
pub(crate) struct Quantization {
    /// Per-component minimum of the original f32 attribute. Length = N.
    pub mins: Vec<f32>,
    /// Range size: shared across components, equals max(per-component max - min).
    pub range: f32,
    /// `2^bits - 1`, precomputed.
    pub max_quantized: f32,
}

impl Quantization {
    pub(crate) fn read<R: ByteReader>(reader: &mut R, n: usize) -> Result<Self, Err> {
        let mut mins = Vec::with_capacity(n);
        for _ in 0..n {
            mins.push(read_f32(reader)?);
        }
        let range = read_f32(reader)?;
        let bits = reader.read_u8()?;
        let max_quantized = ((1u32 << bits).saturating_sub(1)) as f32;
        Ok(Self {
            mins,
            range,
            max_quantized,
        })
    }

    /// Inverse: `f32 = min + (i32 / max_quantized) * range`.
    pub(crate) fn dequantize_into(&self, value: &[i32], out: &mut [f32]) {
        debug_assert_eq!(value.len(), out.len());
        debug_assert_eq!(value.len(), self.mins.len());
        if self.max_quantized == 0.0 || self.range == 0.0 {
            out.copy_from_slice(&self.mins);
            return;
        }
        for i in 0..value.len() {
            let normalized = value[i] as f32 / self.max_quantized;
            out[i] = self.mins[i] + normalized * self.range;
        }
    }
}

/// Octahedral dequantization — inverse of
/// `encode/attribute/portabilization/octahedral_quantization.rs`. Takes
/// 2-component i32 oct-quantized values and produces 3D unit normal f32.
pub(crate) struct OctahedralNormal {
    /// Number of quantization bits the encoder used (it writes a single
    /// u8). Quantization range is `(2^(bits-1) - 1)`, mirroring the
    /// encoder's `(1 << (bits-1)) - 1` factor.
    pub max_quantized: f32,
}

impl OctahedralNormal {
    pub(crate) fn read<R: ByteReader>(reader: &mut R) -> Result<Self, Err> {
        let bits = reader.read_u8()?;
        if bits == 0 || bits > 31 {
            return Err(Err::OctahedralBitsOutOfRange(bits));
        }
        let max_quantized = ((1u32 << (bits - 1)) - 1) as f32;
        Ok(Self { max_quantized })
    }

    /// `oct_i32[2] → unit_normal_f32[3]`. The encoder did:
    /// 1. `octahedral_transform(normal)` → `[u, v]` in `[-1, 1]`
    /// 2. `+ (1, 1)` → `[0, 2]`
    /// 3. `* max_quantized` → `i32`
    ///
    /// We invert each step then call `octahedral_inverse_transform`.
    pub(crate) fn dequantize(&self, value: &[i32; 2]) -> [f32; 3] {
        if self.max_quantized == 0.0 {
            return [0.0, 0.0, 1.0];
        }
        let u = (value[0] as f32) / self.max_quantized - 1.0;
        let v = (value[1] as f32) / self.max_quantized - 1.0;
        octahedral_inverse_transform(u, v)
    }
}

/// 2D octahedral coordinate `(u, v)` in `[-1, 1]` → unit 3D normal.
/// Mirrors Google's `OctahedronToolBox::OctahedralCoordsToUnitVector`
/// (`normal_compression_utils.h`), bit-for-bit.
///
/// `x = 1 - |y| - |z|` is signed: positive for right-hemisphere normals
/// (point in the diamond), negative for left-hemisphere (point outside
/// the diamond). The earlier float-only port that mirror-folded `(y, z)`
/// in the negative-x branch produced the right MAGNITUDE of x but
/// always-positive SIGN — so left-hemisphere normals came out with
/// flipped x. That bug only surfaced once we ran a per-vertex value
/// compare against Google on Skyline 3D Tiles (the existing
/// nearest-neighbor "permutation-tolerant" tests matched any expected
/// normal close to ours, so a sign flip on x was hidden).
pub(crate) fn octahedral_inverse_transform(u: f32, v: f32) -> [f32; 3] {
    let mut y = u;
    let mut z = v;
    // x is signed — positive in the diamond (right hemisphere),
    // negative outside (left hemisphere).
    let x = 1.0 - y.abs() - z.abs();
    // x_offset = max(0, -x). For right hemisphere x ≥ 0, x_offset = 0
    // (the y/z update below is a no-op). For left hemisphere x < 0,
    // x_offset = -x > 0 and we mirror y/z along the nearest diamond
    // edge to recover the unfolded octahedron coords.
    let x_offset = (-x).max(0.0);
    y += if y < 0.0 { x_offset } else { -x_offset };
    z += if z < 0.0 { x_offset } else { -x_offset };
    let norm_squared = x * x + y * y + z * z;
    if norm_squared < 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    let d = 1.0 / norm_squared.sqrt();
    [x * d, y * d, z * d]
}

/// Forward 2D octahedral transform: 3D normal `(x, y, z)` → `(u, v)` in
/// `[-1, 1]`. Inverse of `octahedral_inverse_transform`.
/// Mirrors `encode/attribute/prediction_transform/geom.rs::octahedral_transform`.
#[allow(dead_code)] // Float oct transform, kept alongside the int port for reference.
pub(crate) fn octahedral_transform_f32(normal: [f32; 3]) -> [f32; 2] {
    let abs_sum = normal[0].abs() + normal[1].abs() + normal[2].abs();
    if abs_sum == 0.0 {
        return [0.0, 0.0];
    }
    let mut u = normal[1] / abs_sum;
    let mut v = normal[2] / abs_sum;
    if normal[0] < 0.0 {
        let u_out = if u < 0.0 {
            v.abs() - 1.0
        } else {
            1.0 - v.abs()
        };
        let v_out = if v < 0.0 {
            u.abs() - 1.0
        } else {
            1.0 - u.abs()
        };
        u = u_out;
        v = v_out;
    }
    [u, v]
}

// Reserved for the `octahedral_transform_f32` axis-stable variant. Kept
// because `f32::signum` returns ±1 for ±0.0 which destabilizes the
// octahedral inverse on unit-vector axes; this trait gives the +1
// fallback we'd want there.
#[allow(dead_code)]
trait SignumOrOne {
    fn signum_or_one(self) -> Self;
}

#[allow(dead_code)]
impl SignumOrOne for f32 {
    fn signum_or_one(self) -> Self {
        if self < 0.0 {
            -1.0
        } else {
            1.0
        }
    }
}

fn read_f32<R: ByteReader>(reader: &mut R) -> Result<f32, ReaderErr> {
    let bytes = [
        reader.read_u8()?,
        reader.read_u8()?,
        reader.read_u8()?,
        reader.read_u8()?,
    ];
    Ok(f32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oct_inverse_of_canonical_axis_normals() {
        // (0, 0) is the +x axis after the encoder's transform.
        let n = octahedral_inverse_transform(0.0, 0.0);
        assert!((n[0] - 1.0).abs() < 1e-5, "+x axis: {:?}", n);
    }

    #[test]
    fn oct_normal_round_trip_quant() {
        // Sanity: a normal at quant midpoint with default 8-bit
        // (max=127) should deport to roughly the +x direction.
        let dq = OctahedralNormal {
            max_quantized: 127.0,
        };
        // u = v = 0 → +x axis
        let mid = [127i32, 127i32];
        let n = dq.dequantize(&mid);
        assert!((n[0] - 1.0).abs() < 1e-3, "got {:?}", n);
    }
}
