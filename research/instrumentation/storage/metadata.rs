impl MetadataIndex {
    #[doc(hidden)]
    pub fn research_storage(&self) -> serde_json::Value {
        use serde_json::json;
        use std::mem::size_of;
        json!({"split":self.lexical.research_storage(),"identifiers":self.identifiers.research_storage(),
            "nodes":self.nodes.len(),"node_inline_capacity_bytes":self.nodes.capacity()*size_of::<Node>(),
            "nodes_json_bytes_not_heap":serde_json::to_vec(&self.nodes).unwrap().len(),
            "bare":crate::research_storage::lists(self.bare.iter().map(|(k,v)| (k.as_str(),v.len(),v.capacity())),size_of::<usize>()),
            "qualified":crate::research_storage::lists(self.qualified.iter().map(|(k,v)| (k.as_str(),v.len(),v.capacity())),size_of::<usize>()),
            "folded":crate::research_storage::lists(self.folded.iter().map(|(k,v)| (k.as_str(),v.len(),v.capacity())),size_of::<usize>()),
            "symbol_ordinal_capacity_bytes":self.symbols.capacity()*size_of::<usize>(),
            "file_ordinal_capacity_bytes":self.file_ordinals.capacity()*size_of::<usize>()})
    }
}
