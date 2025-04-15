pub(crate) fn splice_disjoint_indeces(set_of_indeces: Vec<Vec<std::ops::Range<usize>>>) -> Vec<std::ops::Range<usize>> {
    let mut spliced = set_of_indeces.into_iter()
        .flatten()
        .collect::<Vec<_>>();

    spliced.sort_by(|a, b| a.start.cmp(&b.start));

    // ToDo: connect the adjacent ranges
    spliced
}