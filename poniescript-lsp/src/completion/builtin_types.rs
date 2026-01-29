use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

pub fn builtin_types(completions: &mut Vec<CompletionItem>) {
    completions.push(CompletionItem {
        label: "int".into(),
        kind: Some(CompletionItemKind::STRUCT),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "float".into(),
        kind: Some(CompletionItemKind::STRUCT),
        ..Default::default()
    });
    for i in 2..=4 {
        completions.push(CompletionItem {
            label: format!("vec{}", i),
            kind: Some(CompletionItemKind::STRUCT),
            ..Default::default()
        });
        completions.push(CompletionItem {
            label: format!("vec{}i", i),
            kind: Some(CompletionItemKind::STRUCT),
            ..Default::default()
        });
    }
    completions.push(CompletionItem {
        label: "Str".into(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "StrBuf".into(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "StrConst".into(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "Array".into(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "DynArray".into(),
        kind: Some(CompletionItemKind::CLASS),
        ..Default::default()
    });
}