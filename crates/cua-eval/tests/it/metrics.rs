//! The arithmetic of the report: intervals and percentiles.

use cua_eval::metrics::{Permille, percentile, wilson};

/// `(row, trials, hits, point, low, high)`, all in permille.
const WILSON_CASES: [(&str, u32, u32, u32, u32, u32); 4] = [
    ("no trials know nothing: the whole range", 0, 0, 0, 0, 1000),
    ("zero in ten still allows a quarter", 10, 0, 0, 0, 278),
    ("all in ten is the mirror of none", 10, 10, 1000, 722, 1000),
    (
        "half of a hundred is about forty to sixty",
        100,
        50,
        500,
        404,
        596,
    ),
];

#[test]
fn the_wilson_interval_of_known_counts() {
    for (row, trials, hits, point, low, high) in WILSON_CASES {
        let r = wilson(trials, hits);
        assert_eq!(
            (r.point, r.low, r.high),
            (Permille(point), Permille(low), Permille(high)),
            "{row}"
        );
    }
}

#[test]
fn more_runs_narrow_the_interval() {
    let few = wilson(10, 5);
    let many = wilson(1000, 500);
    assert!(many.high.0 - many.low.0 < few.high.0 - few.low.0);
}

#[test]
fn percentiles_are_nearest_rank() {
    let values = [5, 1, 4, 2, 3, 10, 9, 8, 7, 6];
    assert_eq!(percentile(&values, 50), 5);
    assert_eq!(percentile(&values, 95), 10);
    assert_eq!(percentile(&values, 100), 10);
    assert_eq!(percentile(&[], 50), 0);
    assert_eq!(percentile(&[7], 95), 7);
}
