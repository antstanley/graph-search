//! The `graph-search` binary.
//!
//! Placeholder: the real command surface lands in milestones M1–M5; see
//! `SPEC.md` §10 for the reference.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

fn main() {
    // The CLI is a thin client over the library; this only proves the edge
    // exists until the command surface lands in M1 (see SPEC.md §10, §17).
    let _index = graph_search::Index;
    eprintln!("graph-search: not yet implemented (see SPEC.md §17, milestone M1)");
}
