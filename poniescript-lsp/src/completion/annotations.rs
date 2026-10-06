use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

pub fn annotation_completions(completions: &mut Vec<CompletionItem>) {
    completions.push(CompletionItem {
        label: "@inner".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("annotates a class that should have a pointer to its parent class".into()),
        ..Default::default()
    });
}