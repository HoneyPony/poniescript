use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*, expr::*, inf_write, source::*
};

use crate::document::DocumentStore;
use crate::document::*;

struct HoverVisitor<'map> {
    response: Option<Hover>,

    id_to_url_map: &'map HashMap<SourceId, Url>
}

fn build_hover(title: &str, contents: &str) -> Hover {
    Hover {
        contents: HoverContents::Array(
            vec![
                MarkedString::LanguageString(LanguageString {
                    language: "poniescript".to_string(),
                    value: title.to_string()
                }),
                MarkedString::String(contents.to_string())
            ]
        ),
        range: None
    }
}

impl<'map> HoverVisitor<'map> {
    fn build_hover(&mut self, title: &str, contents: &str) {
        self.response = Some(build_hover(title, contents));
    }

    fn hover_class(&mut self, ast: &Ast, db: &Db, class: ClassId) {
        let class = db.get(class);

        let mut class_sig = String::new();
        inf_write!(class_sig, "class {}", db.get(class.name));

        self.build_hover(&class_sig, "");
    }

    fn hover_fun(&mut self, ast: &Ast, db: &Db, fun: FunId) {
        // No unassigned funs...?

        let fun = db.get(fun);

        // TODO: Cache these.
        let mut fun_sig = String::new();
        if let Some(fn_name) = fun.name {
            inf_write!(fun_sig, "fun {}(", db.get(fn_name));
        }
        else {
            inf_write!(fun_sig, "fun(");
        }
        let mut comma = false;
        for param in &fun.parameters {
            if comma { inf_write!(fun_sig, ", "); }

            let param = db.get(*param);
            inf_write!(fun_sig, "{}: {}",
                db.get(param.name), db.repr_type(param.typ));

            comma = true;
        }
        inf_write!(fun_sig, ")");

        if fun.return_type != db.types.void {
            inf_write!(fun_sig, " -> {}", db.repr_type(fun.return_type));
        }

        self.build_hover(&fun_sig, "");
    }

    fn hover_var(&mut self, ast: &Ast, db: &Db, var: VarId) {
        let var = db.get(var);

        let mut var_sig = String::new();
        inf_write!(var_sig, "var {}: {}", db.get(var.name), db.repr_type(var.typ));

        self.build_hover(&var_sig, "");
    }
}

fn cursor_on(cursor: &SourceLocation, target: &SourceLocation) -> bool {
    if cursor.offset < target.offset { return false; }
    if cursor.offset > target.offset + target.length { return false; }
    return true;
}

impl<'a> LocateAst for HoverVisitor<'a> {
    fn locate_assign(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Assign) {
        // TODO: We could just not even do an origin_selection_range here as the
        // default should be correct...?
        if cursor_on(_loc, &it.var_name) {
            self.hover_var(ast, db, it.identity);
        }
    }

    fn locate_variable(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Variable) {
        self.hover_var(ast, db, it.identity);
    }

    fn locate_get(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Get) {
        self.hover_var(ast, db, it.var);
    }

    fn locate_set(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Set) {
        self.hover_var(ast, db, it.var);
    }

    fn locate_new(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &New) {
        eprintln!("it.identifier.location: {} ? {} ? {}",
            it.identifier.location.offset,
            loc.offset,
            it.identifier.location.offset + it.identifier.location.length);
        if cursor_on(loc, &it.identifier.location) {
            self.hover_class(ast, db, it.class);
        }
    }

    fn locate_funcall(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &FunCall) {
        if cursor_on(loc, &it.fn_name) {
            self.hover_fun(ast, db, it.identity)
        }
    }

    fn locate_funcapture(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &FunCapture) {
        if cursor_on(loc, &it.fn_name) {
            self.hover_fun(ast, db, it.identity);
        }
    }
}

pub fn hover(store: &mut DocumentStore, params: HoverParams) -> Option<Hover> {
    let Some(project) = store.projects.get(&params.text_document_position_params.text_document.uri) else {
        return None;
    };

    let cached = project.get_cache(store);
    let cached = cached.lock().unwrap();

    let Some(id) = cached.url_to_id_map.get(&params.text_document_position_params.text_document.uri) else {
        return None;
    };

    let source_loc = inverse_convert_position(&cached.ast, *id, &params.text_document_position_params.position);

    let mut visitor = HoverVisitor {
        response: None,

        id_to_url_map: &cached.id_to_url_map
    };

    visitor.visit_ast(&cached.ast, &cached.db, &source_loc);

    visitor.response
}