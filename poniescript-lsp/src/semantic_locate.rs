use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*,
    expr::*,
    source::*,
};

use crate::document::DocumentStore;
use crate::document::*;

pub enum Semantic {
    Var(VarId),
    Fun(FunId),
    Class(ClassId),
}

// pub struct SemanticRanges<'tok> {
//     origin_selection_range: Option<&'tok SourceLocation>,
//     target_range: &'tok SourceLocation,
//     target_selection_range: &'tok SourceLocation,
// }

struct SemanticLocate<F: FnMut(Semantic, Option<&SourceLocation>)> {
    callback: F,
}

impl<F: FnMut(Semantic, Option<&SourceLocation>)> SemanticLocate<F> {
    fn got_class(&mut self, ast: &Ast, db: &Db, class: ClassId, origin_selection_range: Option<&SourceLocation>) {
        if class == db.class_unassigned {
            eprintln!("no class :(");
            return;
        }
        (self.callback)(Semantic::Class(class), origin_selection_range);
    }

    fn got_fun(&mut self, ast: &Ast, db: &Db, fun: FunId, origin_selection_range: Option<&SourceLocation>) {
        // No unassigned funs...?
        (self.callback)(Semantic::Fun(fun), origin_selection_range);
    }

    fn got_var(&mut self, ast: &Ast, db: &Db, var: VarId, origin_selection_range: Option<&SourceLocation>) {
        // Don't callback for unassigned var
        if var == db.var_unassigned {
            return;
        }
        (self.callback)(Semantic::Var(var), origin_selection_range);
    }
}

fn cursor_on(cursor: &SourceLocation, target: &SourceLocation) -> bool {
    if cursor.offset < target.offset { return false; }
    if cursor.offset > target.offset + target.length { return false; }
    return true;
}

impl<F: FnMut(Semantic, Option<&SourceLocation>)> LocateAst for SemanticLocate<F> {
    fn locate_assign(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Assign) {
        // TODO: We could just not even do an origin_selection_range here as the
        // default should be correct...?
        self.got_var(ast, db, it.identity,  Some(&it.var_name));
    }

    fn locate_variable(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Variable) {
        self.got_var(ast, db, it.identity, Some(&it.location));
    }

    fn locate_get(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Get) {
        //self.goto_var(ast, db, it.var, Some(&it.identifier.location));
    }

    fn locate_set(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Set) {
        //self.goto_var(ast, db, it.var, Some(&it.identifier.location));
    }

    fn locate_new(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &New) {
        if cursor_on(loc, &it.identifier.location) {
            self.got_class(ast, db, it.class, Some(&it.identifier.location));
        }
    }

    fn locate_funcall(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &FunCall) {
        if cursor_on(loc, &it.fn_name) {
            self.got_fun(ast, db, it.identity, Some(&it.fn_name))
        }
    }

    fn locate_funcapture(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &FunCapture) {
        if cursor_on(loc, &it.fn_name) {
            self.got_fun(ast, db, it.identity, Some(&it.fn_name));
        }
    }
}

pub fn semantic_locate<F: FnMut(Semantic, Option<&SourceLocation>)>(ast: &Ast, db: &Db, target: SourceLocation, callback: F) {
    let mut locate = SemanticLocate { callback };
    locate.visit_ast(ast, db, &target);
}
