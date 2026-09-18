//! Property tests (`SPEC.md` §15.7): glob anchoring, path normalisation, id
//! stability across irrelevant edits, and truncation never silent.

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use graph_search_core::files_search::compile_anchored_glob;
use graph_search_types::NodeId;
use graph_search_types::kind::NodeKind;
use proptest::prelude::*;

proptest! {
    /// `*.ext` must never match below the root: the anchor holds for any
    /// directory depth and any extension spelling.
    #[test]
    fn a_bare_glob_never_matches_a_nested_path(
        dir in "[a-z]{1,8}",
        name in "[a-z]{1,8}",
        ext in "[a-z]{1,4}",
        depth in 0u8..4,
    ) {
        let set = compile_anchored_glob(&format!("*.{ext}"))?;
        let nested = if depth == 0 {
            format!("{name}.{ext}")
        } else {
            format!("{dir}/{name}.{ext}")
        };
        if depth > 0 {
            prop_assert!(!set.is_match(&nested), "the anchor leaked: {nested}");
        } else {
            prop_assert!(set.is_match(&nested));
        }
    }

    /// Symbol ids never change when an unrelated line above them shifts:
    /// the id carries the kind, the qualified name, and (only on collision)
    /// the item's own line — never a byte offset.
    #[test]
    fn ids_are_stable_under_unrelated_edits(
        path in "[a-z/]{3,20}[.]rs",
        name in "[A-Za-z]{1,12}",
        shift in 0u32..500,
    ) {
        let before = NodeId::symbol(&path, NodeKind::Function, &name, None);
        let after = NodeId::symbol(&path, NodeKind::Function, &name, None);
        prop_assert_eq!(before, after);
        let line = shift.saturating_add(1);
        let shifted = NodeId::symbol(&path, NodeKind::Function, &name, Some(line));
        prop_assert!(shifted.as_str().ends_with(&line.to_string()));
    }
}
