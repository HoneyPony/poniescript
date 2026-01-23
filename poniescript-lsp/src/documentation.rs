use poniescript_core::{db::{Db, IdFuncs}, lexer::Token};
use tower_lsp::lsp_types::{Documentation, MarkupContent, MarkupKind};

/// Converts a vector of tokens into a String.
/// 
/// Not the most efficient, but that's OK.
pub fn inefficient_doc(db: &Db, tokens: &Option<Vec<Token>>) -> Option<String> {
    tokens.as_ref().map(|tokens| {
        let mut doc = String::new();

        for tok in tokens {
            doc.push_str(db.get(tok.lexeme));
        }

        doc
    })
}

pub fn inefficient_doc_lsp(db: &Db, tokens: &Option<Vec<Token>>) -> Option<Documentation> {
    inefficient_doc(db, tokens).map(|s| Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: s,
    }))
}