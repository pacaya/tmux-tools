//! The first-changed-line rule of a History mark that is a full read taken before
//! submission (PRD-261109-2223-01 P5.1), shared by the Herdr and Rex backends
//! (PRD-261112-2108-01 P5.2).

/// The smallest line index at which `after` differs from the mark read `mark`, where a
/// line that only one read has counts as different; the number of lines of the mark
/// read when the reads are equal.
pub(crate) fn first_changed_line<S: AsRef<str>>(mark: &[S], after: &[&str]) -> usize {
    (0..mark.len().max(after.len()))
        .find(|&index| mark.get(index).map(AsRef::as_ref) != after.get(index).copied())
        .unwrap_or(mark.len())
}

/// Whether the first changed line lies within the last `rows` lines of the mark read:
/// it is at least the number of lines of the mark read minus `rows`.
pub(crate) fn within_last_rows(first_changed: usize, mark_lines: usize, rows: usize) -> bool {
    first_changed >= mark_lines.saturating_sub(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_changed_line_follows_p5_1() {
        let mark = ["a", "b", "c"];
        assert_eq!(first_changed_line(&mark, &["a", "b", "c", "d"]), 3);
        assert_eq!(first_changed_line(&mark, &["a", "x", "c"]), 1);
        assert_eq!(first_changed_line(&mark, &["a"]), 1);
        assert_eq!(first_changed_line(&mark, &["a", "b", "c"]), 3);
        assert!(within_last_rows(1, 3, 2));
        assert!(!within_last_rows(0, 3, 2));
    }
}
