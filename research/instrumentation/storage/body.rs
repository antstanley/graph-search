impl BodyIndex {
    #[doc(hidden)]
    pub fn research_storage(&self) -> serde_json::Value {
        use serde_json::json;
        use std::mem::size_of;
        json!({"postings":crate::research_storage::lists(self.postings.iter().map(|(k,v)| (k.as_str(),v.len(),v.capacity())),size_of::<Posting>()),
            "docid_encoding":crate::research_storage::deltas(self.postings.values().map(|v| v.iter().map(|p| p.document).collect())),
            "posting_field_bytes":2*size_of::<usize>()+size_of::<u32>(),
            "documents":self.documents.len(),"document_size":size_of::<Document>(),
            "document_length_bytes":self.documents.len()*size_of::<Document>(),
            "document_capacity_bytes":self.documents.capacity()*size_of::<Document>(),
            "files":self.files.len(),"file_path_utf8_bytes":self.files.iter().map(String::len).sum::<usize>(),
            "identifier_lane":self.identifiers.as_ref().map(|index|index.research_storage())})
    }
}
