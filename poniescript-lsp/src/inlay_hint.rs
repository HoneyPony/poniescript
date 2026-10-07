use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*, expr::*
};
use poni_arena::IndexCell;

use std::sync::Arc;

use crate::document::{DocumentStore, Project};

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
impl poniescript_core::expr::VisitAstImmut for InlayHintVisitor {
    fn visit_declare(&mut self,ast: &Ast, db: &Db, id:StmtId) {
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
                padding_left: Some(false),
                padding_right: Some(false),
                data: None,
            };

            self.cache.hints.push(hint);
        }

        if let Some(value) = declare.value {
            self.visit_expr(ast, db, value);
        }
    }

    fn visit_fundeclare(&mut self, ast: &Ast, db: &Db, id: ExprId) {
        let binding = ast.get_expr(id);
        let declare = into!(binding.as_ref(), FunDeclare);

        let fun = db.get(declare.identity);
        for i in 0..fun.parameters.len() {
            if declare.untyped_params.contains(i) {
                let var = db.get(fun.parameters[i]);

                let position = var.location.end();
                let (line, col) = ast.sources.get(position.source).get_line_column(&position);
                let (line, col) = ((line - 1) as u32, (col - 1) as u32);
                let position = Position { line, character: col };

                // TODO: Re-use db type repr's somehow...?
                let label = format!(": {}", db.repr_type(var.typ));

                let hint = InlayHint {
                    position,
                    label: InlayHintLabel::String(label),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(false), // TODO: Maybe make the padding configurable?
                    padding_right: Some(false),
                    data: None,
                };

                self.cache.hints.push(hint);
            }
        }

        // This parameter index is treated as corresponding to the return type.
        // TODO: Allow configuring these. I like rust-analyzer's options, where they can either be off,
        // only on lambdas with blocks (e.g. |a, b|` -> int` {}), or always on. And of course the rest of
        // the inlay hints we do should be optional as well.
        if declare.untyped_params.contains(fun.parameters.len())
            && fun.return_type != db.types.void // Don't generate them for -> void as that's redundant
        {
            let mut position = declare.location.begin();
            position.offset += declare.ret_boundary as u64 - 1; // This seems to give the right offset..

            let (line, col) = ast.sources.get(position.source).get_line_column(&position);
            let (line, col) = ((line - 1) as u32, (col - 1) as u32);
            let position = Position { line, character: col };

            // TODO: Re-use db type repr's somehow...?
            let label = format!(" -> {}", db.repr_type(fun.return_type));

            let hint = InlayHint {
                position,
                label: InlayHintLabel::String(label),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(false),
                data: None,
            };

            self.cache.hints.push(hint);
        }


    }
}

pub struct InlayHintCache {
    pub hints: Vec<InlayHint>,
}

impl InlayHintCache {
    pub fn empty() -> Self {
        InlayHintCache { hints: vec![] }
    }
}

pub fn compute_inlay_hint_cache(store: &DocumentStore, project: &Arc<Project>, url: &Url) -> Option<InlayHintCache> {
    let cache = InlayHintCache::empty();

    //self.client.log_message(MessageType::INFO, format!("Semantic tokens requested for {}", params.text_document.uri)).await;

    // TODO: Yep, this is horrible.
    let proj = project.get_cache(store);
    let proj = proj.lock().unwrap();

    let Some(id) = proj.url_to_id_map.get(url) else {
        return None;
    };

    let mut visitor = InlayHintVisitor { cache };

    // TODO: Implement another Visitor that lets us iterate over everything
    // in a module. (and everyting in an Ast.)
    let source = proj.ast.sources.get(*id);
    let module = &source.module;
    for fun in &module.functions {
        visitor.visit_expr(&proj.ast, &proj.db, fun.value);
    }
    for class in &module.classes {
        for fun in &class.funs {
            visitor.visit_expr(&proj.ast, &proj.db, fun.value);
        }
    }

    Some(visitor.cache)
}