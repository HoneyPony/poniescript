use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionItemLabelDetails};

pub fn keyword_completions(completions: &mut Vec<CompletionItem>) {
    completions.push(CompletionItem {
        label: "return".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(": Never".into()),
            description: None
        }),
        detail: Some("return a value from the current function".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "break".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(": Never".into()),
            description: None
        }),
        detail: Some("break from the current loop".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "continue".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(": Never".into()),
            description: None
        }),
        detail: Some("jump to the next iteration of this loop, skipping the rest of the current iteration".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "if".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("conditional expression".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "else".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("else branch for if, or for optional types".into()),
        ..Default::default()
    });

    completions.push(CompletionItem {
        label: "loop".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("infinite loop".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "while".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("loop while a condition is true".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "for".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("loop over an iterable".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "in".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("partial for loop".into()),
        ..Default::default()
    });

    completions.push(CompletionItem {
        label: "class".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("class declaration".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "fun".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("function declaration".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "var".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("variable declaration".into()),
        ..Default::default()
    });

    completions.push(CompletionItem {
        label: "self".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("the current object".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "new".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("construct a new object".into()),
        ..Default::default()
    });

    completions.push(CompletionItem {
        label: "true".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(": bool".into()),
            description: None
        }),
        detail: Some("literal for boolean true".into()),
        ..Default::default()
    });
    completions.push(CompletionItem {
        label: "false".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(": bool".into()),
            description: None
        }),
        detail: Some("literal for boolean true".into()),
        ..Default::default()
    });

    completions.push(CompletionItem {
        label: "nil".into(),
        kind: Some(CompletionItemKind::KEYWORD),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(": T?".into()),
            description: None
        }),
        detail: Some("literal for optional none".into()),
        ..Default::default()
    });
}