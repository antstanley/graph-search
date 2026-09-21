impl GrafeoStore {
    #[doc(hidden)]
    pub fn research_storage(&self) -> serde_json::Value {
        use serde_json::json;
        let mut terms = 0usize;
        let mut identifiers = 0usize;
        let mut lines = 0usize;
        let mut line_capacity = 0usize;
        let mut units = 0usize;
        for file in self.sources.values() {
            units += file.units.len();
            for unit in &file.units {
                terms += unit.terms.keys().map(String::len).sum::<usize>();
                identifiers += unit.identifiers.keys().map(String::len).sum::<usize>();
                for values in unit.terms.values().chain(unit.identifiers.values()) {
                    lines += values.len(); line_capacity += values.capacity();
                }
            }
        }
        json!({"metadata":self.metadata.research_storage(),"body":self.body.research_storage(),
            "adjacency":self.adjacency.research_storage(),
            "source_facts":{"files":self.sources.len(),"units":units,
                "term_utf8_bytes":terms,"identifier_utf8_bytes":identifiers,
                "line_occurrences":lines,"line_length_bytes":lines*4,"line_capacity_bytes":line_capacity*4,
                "json_bytes_not_heap":serde_json::to_vec(&self.sources).unwrap().len()},
            "occurrence_facts":{"files":self.occurrence_files.len(),"records":self.occurrence_files.values().map(|f|f.records.len()).sum::<usize>(),
                "json_bytes_not_heap":serde_json::to_vec(&self.occurrence_files).unwrap().len()},
            "note":"Partial composition: neither JSON bytes nor inline sizes are complete heap estimates. No token-position stream or source-text blob retained in these native postings. Grafeo internals, map nodes, record strings and allocator overhead excluded."})
    }
}
