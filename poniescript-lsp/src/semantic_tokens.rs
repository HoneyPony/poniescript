use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*, expr::*, source::SourceLocation
};
use poni_arena::IndexCell;

use std::sync::Arc;

use crate::document::DocumentStore;

struct SemanticTokenVisitor {
    tokens: Vec<SemanticToken>,
    cursor_start: u64,
    cursor_line: u64,
}

impl SemanticTokenVisitor {
    fn push_token(&mut self, ast: &Ast, _db: &Db, location: &SourceLocation, token_type: u32, token_modifiers_bitset: u32) {
        let (line, col) = ast.sources.get(location.source).get_line_column(location);
        let (line, col) = (line - 1, col - 1);

        let mut delta_line: u32 = 0;
        let delta_start: u32;

        if line == self.cursor_line {
            if col < self.cursor_start {
                eprintln!("bad semantic token");
                return;
            }
            delta_start = (col - self.cursor_start) as u32;
        }
        else {
            if line < self.cursor_line {
                // TODO: Apparently our visit is not necessarily in-order.
                // We need to do stuff to fix that (probably sort all the tokens)
                eprintln!("bad semantic token");
                return;
            }
            delta_line = (line - self.cursor_line) as u32;
            delta_start = col as u32;
        }

        self.cursor_line = line;
        self.cursor_start = col;

        eprintln!("{}:{}: length: {}", self.cursor_line, self.cursor_start, location.length);

        self.tokens.push(SemanticToken { delta_line, delta_start, length: location.length as u32, token_type, token_modifiers_bitset });
    }

    // Used for token modifiers. Must line up with what we tell the text editor.
    const MODIFIER_BIT_READONLY: u32 = 1;

    fn push_var(&mut self, ast: &Ast, db: &Db, location: &SourceLocation, id: VarId) {
        let is_param = db.get(id).param_for.is_some();
        let is_field = db.get(id).class.is_some();
        let typ = match (is_param, is_field) {
            (_, true) => 3,
            (true, _) => 1,
            _ => 0
        };

        let modifiers = if db.get(id).readonly { Self::MODIFIER_BIT_READONLY } else { 0 };

        self.push_token(ast, db, location, typ, modifiers);
    }

    fn push_fun(&mut self, ast: &Ast, db: &Db, location: &SourceLocation) {
        eprintln!("push fun: {}", location.length);
        self.push_token(ast, db, location, 2, 0);
    }

    fn visit_fundeclare_any(&mut self, ast: &Ast, db: &Db, decl: &FunDeclare) {
        // Apparently, there is currently no token for the function name itself.
        self.visit_expr(ast, db, decl.value);

        // We could also visit the params but I don't know if there's any point...?
    }

    fn visit_declare_any(&mut self, ast: &Ast, db: &Db, declare: &Declare) {
        self.push_var(ast, db, &declare.ident, declare.identity);
        if let Some(value) = declare.value {
            self.visit_expr(ast, db, value);
        }
    }

    fn visit_classdeclare_any(&mut self, ast: &Ast, db: &Db, decl: &ClassDeclare) {
        for decl in &decl.classes {
            self.visit_classdeclare_any(ast, db, decl);
        }

        for decl in &decl.vars {
            self.visit_declare_any(ast, db, decl);
        }

        for decl in &decl.funs {
            self.visit_fundeclare_any(ast, db, decl);
        }
    }
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
impl poniescript_core::expr::VisitAstImmut for SemanticTokenVisitor {
    fn visit_assign(&mut self, ast: &Ast, db: &Db, id: ExprId) {
        // TODO: Visit nested
        let binding = ast.get_expr(id);
        let assign = into!(binding.as_ref(), Assign);

        self.push_var(ast, db, &assign.var_name, assign.identity);
        self.visit_expr(ast, db, assign.value);
    }

    fn visit_variable(&mut self,ast: &Ast, db: &Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let var = into!(binding.as_ref(), Variable);

        self.push_var(ast, db, &var.location, var.identity);
    }

    fn visit_declare(&mut self, ast: &Ast, db: &Db, id: StmtId) {
        let binding = ast.get_stmt(id);
        let declare = into_stmt!(binding.as_ref(), Declare);

        self.visit_declare_any(ast, db, declare);
    }

    fn visit_fundeclare(&mut self, ast: &Ast, db: &Db, id: ExprId) {
        let binding = ast.get_expr(id);
        let decl = into!(binding.as_ref(), FunDeclare);

        self.visit_fundeclare_any(ast, db, decl);
    }

    fn visit_funcapture(&mut self,ast: &Ast, db: &Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let capt = into!(binding.as_ref(), FunCapture);

        self.push_fun(ast, db, &capt.fn_name);
    }

    fn visit_funcall(&mut self,ast: &Ast, db: &Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let call = into!(binding.as_ref(), FunCall);

        self.push_fun(ast, db, &call.fn_name);

        for arg in &call.args {
            self.visit_expr(ast, db, *arg);
        }
    }

    fn visit_get(&mut self, ast: &Ast, db: &Db, id: ExprId) {
        let binding = ast.get_expr(id);
        let get = into!(binding.as_ref(), Get);

        self.visit_expr(ast, db, get.lhs);

        for (tok, var) in get.chain.iter().zip(get.vars.iter()) {
            self.push_var(ast, db, &tok.location, *var);
        }
    }

    fn visit_set(&mut self, ast: &Ast, db: &Db, id: ExprId) {
        let binding = ast.get_expr(id);
        let set = into!(binding.as_ref(), Set);

        self.visit_expr(ast, db, set.lhs);

        for (tok, var) in set.chain.iter().zip(set.vars.iter()) {
            self.push_var(ast, db, &tok.location, *var);
        }

        self.visit_expr(ast, db, set.rhs);
    }

    fn visit_classdeclare(&mut self, ast: &Ast, db: &Db, id: StmtId) {
        let binding = ast.get_stmt(id);
        let decl = into_stmt!(binding.as_ref(), ClassDeclare);

        self.visit_classdeclare_any(ast, db, decl);
    }
}

pub fn semantic_tokens(store: &mut DocumentStore, params: SemanticTokensParams) -> Option<SemanticTokensResult> {
    // let mut lock = self.store.lock().await;
    
    // let (db, ast, ..) = lock.get_cached_stuff();
    let Some(project) = store.projects.get(&params.text_document.uri) else {
        return None;
    };

    let cached = project.get_cache(store);
    let cached = cached.lock().unwrap();

    let Some(id) = cached.url_to_id_map.get(&params.text_document.uri) else {
        return None;
    };

    let mut visitor = SemanticTokenVisitor { tokens: vec![], cursor_line: 0, cursor_start: 0 };

    visitor.visit_ast_for_source(&cached.ast, &cached.db, *id);
    // for source in ast.sources.iter() {
    //     let source = ast.sources.get(source);
    //     let module = &source.module;
    //     for fun in &module.functions {
    //         visitor.visit_expr(&ast, db, fun.value);
    //     }
    // }

    let tokens = SemanticTokens { result_id: None, data: visitor.tokens };

    // self.client.log_message(MessageType::INFO, format!("Found {} semantic tokens", tokens.data.len())).await;
    // Ok(Some(SemanticTokensResult::Tokens(tokens)))
    Some(SemanticTokensResult::Tokens(tokens))
}