use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*,
    expr::*,
    source::*,
    lexer::Token,
};

use crate::document::DocumentStore;
use crate::semantic_locate::Semantic;
use crate::{document::*, semantic_locate};

/// Converts a vector of tokens into a String.
/// 
/// Not the most efficient, but that's OK.
/// 
/// TODO: Deduplicate this...
fn inefficient_doc(db: &Db, tokens: &Option<Vec<Token>>) -> Option<String> {
    tokens.as_ref().map(|tokens| {
        let mut doc = String::new();

        for tok in tokens {
            doc.push_str(db.get(tok.lexeme));
        }

        doc
    })
}

fn inefficient_doc_lsp(db: &Db, tokens: &Option<Vec<Token>>) -> Option<Documentation> {
    inefficient_doc(db, tokens).map(|s| Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: s,
    }))
}

pub fn completion(store: &mut DocumentStore, params: CompletionParams) -> Option<CompletionResponse> {
    let Some(project) = store.projects.get(&params.text_document_position.text_document.uri) else {
        return None;
    };

    let cached = project.get_cache(store);
    let cached = cached.lock().unwrap();

    // let Some(id) = cached.url_to_id_map.get(&params.text_document_position_params.text_document.uri) else {
    //     return None;
    // };

    // As a first effort, we will simply provide a list of every symbol in the
    // project. Later, it would be nice to cut down the number of scopes that
    // are used.

    let mut completions = Vec::new();

    let db = &cached.db;
    for var in db.iter_var() {
        let var = db.get(var);

        let kind = match var.class {
            Some(_) => CompletionItemKind::FIELD,
            None    => CompletionItemKind::VARIABLE,
        };

        completions.push(CompletionItem {
            label: db.get(var.name).to_string(),
            label_details: Some(CompletionItemLabelDetails {
                detail: Some(format!(": {}", db.repr_type(var.typ))),
                description: None
            }),
            kind: Some(kind),
            documentation: inefficient_doc_lsp(db, &var.doc_comment),
            ..Default::default()
        })
    }

    for fun in db.iter_fun() {
        let fun = db.get(fun);
        let Some(name) = fun.name else { continue; };

        let kind = match fun.class {
            Some(_) => CompletionItemKind::METHOD,
            None    => CompletionItemKind::FUNCTION,
        };

        completions.push(CompletionItem {
            label: db.get(name).to_string(),
            label_details: Some(CompletionItemLabelDetails {
                detail: Some(format!(" {}", db.repr_sig(fun.sig).to_string())),
                description: None
            }),
            kind: Some(kind),
            documentation: inefficient_doc_lsp(db, &fun.doc_comment),
            ..Default::default()
        })
    }

    for class in db.iter_class() {
        let class = db.get(class);
        
        completions.push(CompletionItem {
            label: db.get(class.name).to_string(),
            kind: Some(CompletionItemKind::CLASS),
            documentation: inefficient_doc_lsp(db, &class.doc_comment),
            ..Default::default()
        })
    }

    let mut print = CompletionItem::new_simple("print".to_string(), "Print out any series of expressions.".to_string());
    print.kind = Some(CompletionItemKind::FUNCTION);
    let mut str = CompletionItem::new_simple("str".to_string(), "Convert any series of expressions to a new StrBuf.".to_string());
    str.kind = Some(CompletionItemKind::FUNCTION);

    completions.push(print);
    completions.push(str);

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

    // Keywords
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



    Some(CompletionResponse::Array(completions))
}