impl LexicalIndex {
    #[doc(hidden)]
    pub fn research_storage(&self) -> serde_json::Value {
        use serde_json::json;
        use std::mem::size_of;
        json!({"postings":crate::research_storage::lists(self.postings.iter().map(|(k,v)| (k.as_str(),v.len(),v.capacity())),size_of::<Posting>()),
            "docid_encoding":crate::research_storage::deltas(self.postings.values().map(|v| v.iter().map(|p| p.document).collect())),
            "posting_field_bytes":size_of::<usize>()+size_of::<f32>()+2*size_of::<[u32;4]>(),
            "zero_whole_frequency_postings":self.postings.values().flat_map(|v|v.iter()).filter(|p| p.whole_fields == [0;4]).count(),
            "norm_length_bytes":self.lengths.len()*size_of::<usize>()+self.field_lengths.len()*size_of::<[usize;4]>()+self.whole_field_lengths.len()*size_of::<[usize;4]>(),
            "norm_capacity_bytes":self.lengths.capacity()*size_of::<usize>()+self.field_lengths.capacity()*size_of::<[usize;4]>()+self.whole_field_lengths.capacity()*size_of::<[usize;4]>(),
            "documents":self.lengths.len(),
            "arc_payload_header_bytes":self.postings.len()*(size_of::<Vec<Posting>>()+2*size_of::<usize>()),
            "arc_map_pointer_bytes":self.postings.len()*size_of::<Arc<Vec<Posting>>>(),
            "note":"List Vec headers are inside Arc payload, not map values; do not add both header figures. BTree nodes, allocator overhead and String spare capacity excluded."})
    }
}
