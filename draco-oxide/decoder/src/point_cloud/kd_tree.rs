//! The integer kd-tree point decoder.

use super::bit_decoder::{BitDecoder, DirectBitDecoder, FoldedBitDecoder, RansBitDecoder};
use crate::Err;
use draco_oxide_core::bit_coder::Reader;

/// Decodes the kd-tree point block into a flat `num_points * dimension` array.
pub(super) fn decode_points(
    reader: &mut Reader<'_>,
    dimension: usize,
    expected_points: usize,
    compression_level: u8,
) -> Result<Vec<u32>, Err> {
    match compression_level {
        0 | 1 => decode_typed::<DirectBitDecoder>(reader, dimension, expected_points, false),
        2 | 3 => decode_typed::<RansBitDecoder>(reader, dimension, expected_points, false),
        4 | 5 => decode_typed::<FoldedBitDecoder>(reader, dimension, expected_points, false),
        6 => decode_typed::<FoldedBitDecoder>(reader, dimension, expected_points, true),
        _ => Err(Err::MalformedAttribute(
            "unsupported kd-tree compression level",
        )),
    }
}

struct Status {
    num_remaining_points: u32,
    last_axis: u32,
    stack_pos: usize,
}

fn decode_typed<'a, Num: BitDecoder<'a>>(
    reader: &mut Reader<'a>,
    dimension: usize,
    expected_points: usize,
    select_axis: bool,
) -> Result<Vec<u32>, Err> {
    let bit_length = reader.read_u32()?;
    if bit_length > 32 {
        return Err(Err::MalformedAttribute("kd-tree bit length exceeds 32"));
    }
    let num_points = reader.read_u32()? as usize;
    if num_points != expected_points {
        return Err(Err::MalformedAttribute(
            "kd-tree point count disagrees with the geometry section",
        ));
    }
    if num_points == 0 {
        return Ok(Vec::new());
    }

    let mut numbers = Num::start(reader)?;
    let mut remaining_bits = DirectBitDecoder::start(reader)?;
    let mut axis_decoder = DirectBitDecoder::start(reader)?;
    let mut half = DirectBitDecoder::start(reader)?;

    let dim = dimension;
    // Both counts come from the stream, so the allocations they size must fail
    // as errors rather than abort.
    let total = num_points
        .checked_mul(dim)
        .ok_or(Err::AllocationTooLarge("point array"))?;
    let mut out: Vec<u32> = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| Err::AllocationTooLarge("point array"))?;
    // One base/levels row per tree depth; a split at stack_pos writes the upper
    // child's row at stack_pos + 1.
    let depth = dim
        .checked_mul(32)
        .and_then(|d| d.checked_add(1))
        .ok_or(Err::AllocationTooLarge("kd-tree stack"))?;
    let rows = depth
        .checked_mul(dim)
        .ok_or(Err::AllocationTooLarge("kd-tree stack"))?;
    let mut base_stack = try_zeroed(rows)?;
    let mut levels_stack = try_zeroed(rows)?;
    let mut axes = vec![0u32; dim];
    let mut point = vec![0u32; dim];

    let mut stack = Vec::with_capacity(depth + 1);
    stack.push(Status {
        num_remaining_points: num_points as u32,
        last_axis: 0,
        stack_pos: 0,
    });

    while let Some(status) = stack.pop() {
        let n = status.num_remaining_points;
        let stack_pos = status.stack_pos;
        let sp = stack_pos * dim;

        if n as usize > num_points {
            return Err(Err::MalformedAttribute("kd-tree split count overflow"));
        }

        let axis = if !select_axis {
            increment_mod(status.last_axis, dim as u32)
        } else if n < 64 {
            let levels = &levels_stack[sp..sp + dim];
            let mut best = 0u32;
            for a in 1..dim as u32 {
                if levels[best as usize] > levels[a as usize] {
                    best = a;
                }
            }
            best
        } else {
            axis_decoder.decode_lsb32(4)?
        };
        if axis as usize >= dim {
            return Err(Err::MalformedAttribute("kd-tree split axis out of range"));
        }

        let level = levels_stack[sp + axis as usize];

        if bit_length - level == 0 {
            for _ in 0..n {
                out.extend_from_slice(&base_stack[sp..sp + dim]);
            }
            continue;
        }

        if n <= 2 {
            axes[0] = axis;
            for j in 1..dim {
                axes[j] = increment_mod(axes[j - 1], dim as u32);
            }
            for _ in 0..n {
                for &axis in axes.iter().take(dim) {
                    let a = axis as usize;
                    let nbits = bit_length - levels_stack[sp + a];
                    point[a] = if nbits > 0 {
                        remaining_bits.decode_lsb32(nbits)?
                    } else {
                        0
                    };
                    point[a] |= base_stack[sp + a];
                }
                out.extend_from_slice(&point);
            }
            continue;
        }

        if out.len() > total {
            return Err(Err::MalformedAttribute("kd-tree decoded too many points"));
        }

        let num_remaining_bits = bit_length - level;
        let modifier = 1u32 << (num_remaining_bits - 1);
        base_stack.copy_within(sp..sp + dim, sp + dim);
        base_stack[sp + dim + axis as usize] += modifier;

        let incoming_bits = most_significant_bit(n);
        let number = if incoming_bits > 0 {
            numbers.decode_lsb32(incoming_bits)?
        } else {
            0
        };

        let mut first_half = n / 2;
        if first_half < number {
            return Err(Err::MalformedAttribute("kd-tree split deviation overflow"));
        }
        first_half -= number;
        let mut second_half = n - first_half;

        if first_half != second_half && !half.decode_bit() {
            std::mem::swap(&mut first_half, &mut second_half);
        }

        levels_stack[sp + axis as usize] += 1;
        levels_stack.copy_within(sp..sp + dim, sp + dim);
        if first_half > 0 {
            stack.push(Status {
                num_remaining_points: first_half,
                last_axis: axis,
                stack_pos,
            });
        }
        if second_half > 0 {
            stack.push(Status {
                num_remaining_points: second_half,
                last_axis: axis,
                stack_pos: stack_pos + 1,
            });
        }
    }

    if out.len() != total {
        return Err(Err::MalformedAttribute(
            "kd-tree stream decoded a wrong number of points",
        ));
    }
    Ok(out)
}

/// Allocates `len` zeroed words, erroring rather than aborting when the
/// allocator cannot meet the length.
fn try_zeroed(len: usize) -> Result<Vec<u32>, Err> {
    // Reserving first keeps the pages untouched; resizing in place would commit
    // every one of them.
    let mut probe: Vec<u32> = Vec::new();
    probe
        .try_reserve_exact(len)
        .map_err(|_| Err::AllocationTooLarge("kd-tree stack"))?;
    drop(probe);
    Ok(vec![0u32; len])
}

#[inline]
fn increment_mod(i: u32, m: u32) -> u32 {
    if i == m - 1 {
        0
    } else {
        i + 1
    }
}

/// Index of the highest set bit; `n` must be non-zero.
#[inline]
fn most_significant_bit(n: u32) -> u32 {
    31 - n.leading_zeros()
}
