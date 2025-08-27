use std::collections::HashMap;

use tower_lsp::lsp_types::Url;

pub struct Document {
    text: String,
    url: Url
}

pub struct DocumentStore {
    documents: HashMap<Url, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self { documents: HashMap::new() }
    }

    pub fn update(&mut self, url: Url, text: String) {
        self.documents.insert(url.clone(), Document {
            text, url
        });
    }
}