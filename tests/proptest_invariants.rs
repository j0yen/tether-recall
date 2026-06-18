//! Proptest invariants for tether-recall.
//! READ-ONLY: the edit-agent must not modify this file.

use proptest::prelude::*;
use tether_recall::protocol::is_mutating_verb;
use tether_recall::responder::truncate_snippet;

proptest! {
    /// truncate_snippet never exceeds max_chars.
    #[test]
    fn prop_truncate_never_exceeds_max(
        s in ".*",
        max in 1usize..=500,
    ) {
        let result = truncate_snippet(&s, max);
        prop_assert!(
            result.chars().count() <= max,
            "truncated snippet len {} > max {}",
            result.chars().count(),
            max
        );
    }

    /// truncate_snippet is idempotent when applied twice with the same max.
    #[test]
    fn prop_truncate_idempotent(
        s in ".*",
        max in 1usize..=500,
    ) {
        let once = truncate_snippet(&s, max);
        let twice = truncate_snippet(&once, max);
        prop_assert_eq!(once, twice, "truncate_snippet should be idempotent");
    }

    /// is_mutating_verb is stable under case folding.
    #[test]
    fn prop_mutating_verb_case_insensitive(
        verb in "[a-z]{3,10}",
    ) {
        let lower_result = is_mutating_verb(&verb);
        let upper_result = is_mutating_verb(&verb.to_uppercase());
        prop_assert_eq!(
            lower_result,
            upper_result,
            "is_mutating_verb should be case-insensitive for '{verb}'"
        );
    }
}
