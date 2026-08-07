//! The integer kd-tree point encoder. Points are partitioned in place, so the
//! algorithm does not preserve point order.

use super::bit_encoder::{BitEncoder, DirectBitEncoder, FoldedBitEncoder, RansBitEncoder};
use super::Err;
use draco_oxide_core::bit_coder::ByteWriter;

/// Encodes the kd-tree point block. `points` is a flat row-major array of
/// `dimension` values per point and is reordered in place.
pub(super) fn encode_points<W: ByteWriter>(
    points: &mut [u32],
    dimension: usize,
    compression_level: u8,
    writer: &mut W,
) -> Result<(), Err> {
    match compression_level {
        0 | 1 => encode_typed::<W, DirectBitEncoder>(points, dimension, false, writer),
        2 | 3 => encode_typed::<W, RansBitEncoder>(points, dimension, false, writer),
        4 | 5 => encode_typed::<W, FoldedBitEncoder>(points, dimension, false, writer),
        6 => encode_typed::<W, FoldedBitEncoder>(points, dimension, true, writer),
        _ => Err(Err::UnsupportedCompressionLevel(compression_level)),
    }
}

struct Status {
    begin: usize,
    end: usize,
    last_axis: u32,
    stack_pos: usize,
}

struct Encoders<Num: BitEncoder> {
    numbers: Num,
    remaining_bits: DirectBitEncoder,
    axis: DirectBitEncoder,
    half: DirectBitEncoder,
}

fn encode_typed<W: ByteWriter, Num: BitEncoder>(
    points: &mut [u32],
    dim: usize,
    select_axis: bool,
    writer: &mut W,
) -> Result<(), Err> {
    let num_points = points.len() / dim;

    let bit_length = points
        .iter()
        .map(|&v| if v == 0 { 0 } else { 32 - v.leading_zeros() })
        .max()
        .unwrap_or(0);

    writer.write_u32(bit_length);
    writer.write_u32(num_points as u32);
    if num_points == 0 {
        return Ok(());
    }

    let mut enc = Encoders::<Num> {
        numbers: Num::default(),
        remaining_bits: DirectBitEncoder::default(),
        axis: DirectBitEncoder::default(),
        half: DirectBitEncoder::default(),
    };

    encode_internal(points, dim, bit_length, select_axis, num_points, &mut enc);

    enc.numbers.end(writer)?;
    enc.remaining_bits.end(writer)?;
    enc.axis.end(writer)?;
    enc.half.end(writer)?;
    Ok(())
}

fn encode_internal<Num: BitEncoder>(
    points: &mut [u32],
    dim: usize,
    bit_length: u32,
    select_axis: bool,
    num_points: usize,
    enc: &mut Encoders<Num>,
) {
    let depth = 32 * dim + 1;
    let mut base_stack = vec![0u32; depth * dim];
    let mut levels_stack = vec![0u32; depth * dim];
    let mut axes = vec![0u32; dim];
    let mut deviations = vec![0u32; dim];
    let mut num_remaining_bits_per_axis = vec![0u32; dim];

    let mut stack = Vec::with_capacity(depth + 1);
    stack.push(Status {
        begin: 0,
        end: num_points,
        last_axis: 0,
        stack_pos: 0,
    });

    while let Some(status) = stack.pop() {
        let Status {
            begin,
            end,
            last_axis,
            stack_pos,
        } = status;
        let sp = stack_pos * dim;
        let n = (end - begin) as u32;

        let axis = choose_axis(
            points,
            dim,
            bit_length,
            select_axis,
            begin,
            end,
            &base_stack[sp..sp + dim],
            &levels_stack[sp..sp + dim],
            last_axis,
            &mut deviations,
            &mut num_remaining_bits_per_axis,
            &mut enc.axis,
        );

        let level = levels_stack[sp + axis as usize];

        if bit_length - level == 0 {
            continue;
        }

        if n <= 2 {
            axes[0] = axis;
            for j in 1..dim {
                axes[j] = increment_mod(axes[j - 1], dim as u32);
            }
            for p in begin..end {
                for &axis in axes.iter().take(dim) {
                    let a = axis as usize;
                    let nbits = bit_length - levels_stack[sp + a];
                    if nbits > 0 {
                        enc.remaining_bits.encode_lsb32(nbits, points[p * dim + a]);
                    }
                }
            }
            continue;
        }

        let num_remaining_bits = bit_length - level;
        let modifier = 1u32 << (num_remaining_bits - 1);
        base_stack.copy_within(sp..sp + dim, sp + dim);
        base_stack[sp + dim + axis as usize] += modifier;
        let split_value = base_stack[sp + dim + axis as usize];

        let split = partition(points, dim, begin, end, axis as usize, split_value);

        let required_bits = most_significant_bit(n);
        let first_half = (split - begin) as u32;
        let second_half = (end - split) as u32;
        let left = first_half < second_half;

        if first_half != second_half {
            enc.half.encode_bit(left);
        }
        let deviation = if left {
            n / 2 - first_half
        } else {
            n / 2 - second_half
        };
        enc.numbers.encode_lsb32(required_bits, deviation);

        levels_stack[sp + axis as usize] += 1;
        levels_stack.copy_within(sp..sp + dim, sp + dim);
        if split != begin {
            stack.push(Status {
                begin,
                end: split,
                last_axis: axis,
                stack_pos,
            });
        }
        if split != end {
            stack.push(Status {
                begin: split,
                end,
                last_axis: axis,
                stack_pos: stack_pos + 1,
            });
        }
    }
}

/// Picks the split axis, writing it only when the decoder cannot derive it.
#[allow(clippy::too_many_arguments)]
fn choose_axis(
    points: &[u32],
    dim: usize,
    bit_length: u32,
    select_axis: bool,
    begin: usize,
    end: usize,
    base: &[u32],
    levels: &[u32],
    last_axis: u32,
    deviations: &mut [u32],
    num_remaining_bits: &mut [u32],
    axis_encoder: &mut DirectBitEncoder,
) -> u32 {
    if !select_axis {
        return increment_mod(last_axis, dim as u32);
    }
    let size = (end - begin) as u32;
    if size < 64 {
        let mut best = 0usize;
        for a in 1..dim {
            if levels[best] > levels[a] {
                best = a;
            }
        }
        return best as u32;
    }

    for i in 0..dim {
        deviations[i] = 0;
        num_remaining_bits[i] = bit_length - levels[i];
        if num_remaining_bits[i] > 0 {
            let split = base[i] + (1 << (num_remaining_bits[i] - 1));
            let below = (begin..end)
                .filter(|&p| points[p * dim + i] < split)
                .count() as u32;
            deviations[i] = (size - below).max(below);
        }
    }
    let mut max_value = 0;
    let mut best = 0u32;
    for i in 0..dim {
        if num_remaining_bits[i] > 0 && max_value < deviations[i] {
            max_value = deviations[i];
            best = i as u32;
        }
    }
    axis_encoder.encode_lsb32(4, best);
    best
}

/// Moves every point whose `axis` coordinate is below `value` ahead of the
/// rest, returning where the upper half starts.
fn partition(
    points: &mut [u32],
    dim: usize,
    begin: usize,
    end: usize,
    axis: usize,
    value: u32,
) -> usize {
    let mut first = begin;
    let mut last = end;
    loop {
        while first != last && points[first * dim + axis] < value {
            first += 1;
        }
        if first == last {
            return first;
        }
        loop {
            last -= 1;
            if first == last {
                return first;
            }
            if points[last * dim + axis] < value {
                break;
            }
        }
        for c in 0..dim {
            points.swap(first * dim + c, last * dim + c);
        }
        first += 1;
    }
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
