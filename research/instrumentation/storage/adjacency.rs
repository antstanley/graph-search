impl AdjacencyIndex {
    #[doc(hidden)]
    pub fn research_storage(&self) -> serde_json::Value {
        use serde_json::json;
        use std::mem::size_of;
        json!({"edges":self.edges.len(),"edge_inline_capacity_bytes":self.edges.capacity()*size_of::<Edge>(),
            "edges_json_bytes_not_heap":serde_json::to_vec(&self.edges).unwrap().len(),
            "incident":crate::research_storage::lists(self.incident.iter().map(|(k,v)| (k.as_str(),v.len(),v.capacity())),size_of::<usize>())})
    }
}
