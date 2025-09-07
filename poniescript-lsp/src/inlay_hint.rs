use tower_lsp::lsp_types::*;

use poniescript_core::{
    arena::IndexCell, binder, db::*, expr::*, init_ordering, module::{self, Module}, source::*, typecheck, Args
};

use crate::document::DocumentStore;

struct InlayHintVisitor {
    cache: InlayHintCache
}

// TODO: Deduplicate this
macro_rules! into {
    ($value:expr, $variant:ident) => {
        {
            let Expr::$variant(v) = $value else { unreachable!() };
            v
        }
    };
}

macro_rules! into_stmt {
    ($value:expr, $variant:ident) => {
        {
            let Stmt::$variant(v) = $value else { unreachable!() };
            v
        }
    };
}

// TODO: Consider making VisitAst visit each node strongly-typed or something..?
impl poniescript_core::expr::VisitAst for InlayHintVisitor {
    fn visit_declare(&mut self,ast: &Ast, db: &mut Db, id:StmtId) {
        let binding = ast.get_stmt(id);
        let declare = into_stmt!(binding.as_ref(), Declare);

        if !declare.has_explicit_type {
            let position = declare.ident.end();
            let (line, col) = ast.sources.get(position.source).get_line_column(&position);
            let (line, col) = ((line - 1) as u32, (col - 1) as u32);
            let position = Position { line, character: col };

            let typ = db.get(declare.identity).typ;

            // TODO: Re-use db type repr's somehow...?
            let label = format!(": {}", db.repr_type(typ));

            let hint = InlayHint {
                position,
                label: InlayHintLabel::String(label),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: Some(false),
                data: None,
            };

            self.cache.hints.push(hint);
        }

        self.visit_expr(ast, db, declare.value);
    }
}

pub struct InlayHintCache {
    pub hints: Vec<InlayHint>,
}

pub fn compute_inlay_hint_cache(params: InlayHintParams, store: &mut DocumentStore) -> InlayHintCache {
    let cache = InlayHintCache { hints: vec![] };

    //self.client.log_message(MessageType::INFO, format!("Semantic tokens requested for {}", params.text_document.uri)).await;

    let Ok(path) = params.text_document.uri.to_file_path() else {
        //self.client.log_message(MessageType::INFO, format!("Unable to get Path as file: {}", params.text_document.uri)).await;
        return cache;
    };

    // TODO: Yep, this is horrible.
    let (db, ast, ..) = store.get_cached_stuff();

    let mut visitor = InlayHintVisitor { cache };

    // TODO: Implement another Visitor that lets us iterate over everything
    // in a module.
    for source in ast.sources.iter() {
        let source = ast.sources.get(source);
        let module = &source.module;
        for fun in &module.functions {
            visitor.visit_expr(&ast, db, fun.value);
        }
    }

    visitor.cache
}