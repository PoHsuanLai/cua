//! The arithmetic of the report: intervals and percentiles.

use cua_eval::metrics::{Permille, percentile, wilson};

#[test]
fn no_trials_know_nothing_and_the_interval_is_the_whole_range() {
    let r = wilson(0, 0);
    assert_eq!(
        (r.point, r.low, r.high),
        (Permille(0), Permille(0), Permille(1000))
    );
}

#[test]
fn zero_in_ten_still_allows_a_quarter() {
    let r = wilson(10, 0);
    assert_eq!(r.point, Permille(0));
    assert_eq!(r.low, Permille(0));
    assert_eq!(r.high, Permille(278));
}

#[test]
fn all_in_ten_is_the_mirror_of_none() {
    let r = wilson(10, 10);
    assert_eq!((r.low, r.high), (Permille(722), Permille(1000)));
}

#[test]
fn half_of_a_hundred_is_about_forty_to_sixty() {
    let r = wilson(100, 50);
    assert_eq!(r.point, Permille(500));
    assert_eq!((r.low, r.high), (Permille(404), Permille(596)));
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
