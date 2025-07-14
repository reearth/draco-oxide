use crate::prelude::{NdVector, Vector};

pub mod geom;
pub(crate) mod debug;
pub(crate) mod bit_coder;

#[allow(dead_code)] // Remove this when attribute encoder supports multiple groups.
pub(crate) fn splice_disjoint_indices(set_of_subseqs: Vec<Vec<std::ops::Range<usize>>>) -> Vec<std::ops::Range<usize>> {
    let mut spliced = set_of_subseqs.into_iter()
        .flatten()
        .collect::<Vec<_>>();

    spliced.sort_by(|a, b| a.start.cmp(&b.start));

    connect_subsequence(&mut spliced);
    spliced
}


#[allow(dead_code)] // Remove this when attribute encoder supports multiple groups.
pub(crate) fn merge_indices(mut set_of_subseqs: Vec<Vec<std::ops::Range<usize>>>) -> Vec<std::ops::Range<usize>> {
    for sub_seq in set_of_subseqs.iter_mut() {
        connect_subsequence(sub_seq);
    }

    let set_of_subseqs_len = set_of_subseqs.len();
    let mut iters = set_of_subseqs.into_iter()
        .map(|v| v.into_iter())
        .filter_map(|mut it| if let Some(r) = it.next() {
                Some((r, it))
            } else {
                None
            })
        .collect::<Vec<_>>();

    if iters.len() < set_of_subseqs_len {
        // this means that there is an empty iterator, which kills all the other iterators.
        return Vec::new();
    }

    let mut merged = Vec::new();
    'outer: while let Some((idx,_)) = iters.iter()
            .enumerate()
            .max_by_key(|(_, (val,_))| val.start)
    
    {
        let r = iters[idx].0.clone();

        // make sure that all the iterators are at the position where the range ends
        // at least as late as 'r' starts.
        for (s,it) in iters.iter_mut() {
            while s.end < r.start {
                if let Some(val) = it.next() {
                    *s = val;
                } else {
                    // this means that there is some iterator that does not contain
                    // 'r.start', so this is the end of the function.
                    return merged;
                }

                if s.start > r.start {
                    // this means that 'r.start' cannot be contained in the output.
                    // so we should continue with the outer loop.
                    continue 'outer;
                }
            }
        }

        // Now that 'r.start' is a valid start for the merged range.
        debug_assert!(
            iters.iter_mut().all(|(s,_)| s.start <= r.start),
            "r={:?}, current ranges: {:?}",
            r,
            iters.iter().map(|(s,_)| s).collect::<Vec<_>>()
        );
        // Look for the end of the range, which must have the smallest end.
        let end = iters.iter_mut()
            .map(|(s,_)| s.end)
            .min()
            .unwrap();

        merged.push(r.start..end);

        let (r, it) = &mut iters[idx];
        if let Some(val) = it.next() {
            *r = val;
        } else {
            return merged;
        }
    }

    merged
}

fn connect_subsequence(seq: &mut Vec<std::ops::Range<usize>>) {
    let mut idx1 = 0;
    let mut idx2 = 1;
    while idx2 < seq.len() {
        if seq[idx1].end > seq[idx2].start {
            panic!("Ranges must be disjoint, but they are not: {:?}", seq);
        } else if seq[idx1].end == seq[idx2].start {
            // merge the two ranges
            seq[idx1].end = seq[idx2].end;
            seq.remove(idx2);
        } else {
            idx1 += 1;
            idx2 += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splice_disjoint_indices() {
        let set_of_subseqs = vec![
            vec![0..2],
            vec![4..6],
            vec![2..4, 7..9],
        ];
        let result = splice_disjoint_indices(set_of_subseqs);
        assert_eq!(result, vec![0..6, 7..9]);
    }

    #[test]
    fn test_merge_indices() {
        let set_of_subseqs = vec![
            vec![0..1, 1..2, 3..5, 8..10],
            vec![1..3, 4..6, 8..9],
            vec![1..4, 7..9],
            vec![0..79],
        ];
        let result = merge_indices(set_of_subseqs);
        assert_eq!(result, vec![1..2, 8..9]);
    }
}

#[allow(dead_code)] // Remove this when attribute encoder supports multiple groups.
pub(crate) fn splice_disjoint_indeces(set_of_indeces: Vec<Vec<std::ops::Range<usize>>>) -> Vec<std::ops::Range<usize>> {
    let mut spliced = set_of_indeces.into_iter()
        .flatten()
        .collect::<Vec<_>>();

    spliced.sort_by(|a, b| a.start.cmp(&b.start));

    // ToDo: connect the adjacent ranges
    spliced
}

pub(crate) fn to_positive_i32(val: i32) -> i32 {
    if val >= 0 {
        val << 1
    } else {
        (-(val + 1) << 1) + 1
    }
}

pub(crate) fn to_positive_i32_vec<const N: usize>(mut vec: NdVector<N, i32>) -> NdVector<N, i32> 
    where 
        NdVector<N, i32>: Vector<N, Component = i32>,
{
    for i in 0..N {
        *vec.get_mut(i) = to_positive_i32(*vec.get(i));
    }
    vec
}