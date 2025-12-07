use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*,
    expr::*,
    source::*,
};

use crate::document::DocumentStore;
use crate::document::*;

struct GotoDefinitionVisitor<'map> {
    response: Option<GotoDefinitionResponse>,

    id_to_url_map: &'map HashMap<SourceId, Url>
}

impl<'map> GotoDefinitionVisitor<'map> {
    fn set_link(&mut self, ast: &Ast, origin_selection_range: Option<&SourceLocation>, target_range: &SourceLocation, target_selection_range: &SourceLocation) {
        let Some(target_uri) = self.id_to_url_map.get(&target_range.source) else {
            // Nothing found.
            return;
        };
        
        let origin_selection_range = origin_selection_range.map(|r| convert_range(ast, r));
        let target_range = convert_range(ast, target_range);
        let target_selection_range = convert_range(ast, target_selection_range);

        let link = LocationLink {
            origin_selection_range,
            target_uri: target_uri.clone(),
            target_range,
            target_selection_range
        };
        let response = GotoDefinitionResponse::Link(vec![link]);

        self.response = Some(response);
    }

    fn goto_class(&mut self, ast: &Ast, db: &Db, class: ClassId, origin_selection_range: Option<&SourceLocation>) {
        if class == db.class_unassigned {
            eprintln!("no class :(");
            return;
        }

        let class = db.get(class);

        // Target selection range TODO.

        self.set_link(ast, origin_selection_range,
            &class.location,
            &class.location);
    }

    fn goto_var(&mut self, ast: &Ast, db: &Db, var: VarId, origin_selection_range: Option<&SourceLocation>) {
        if var == db.var_unassigned {
            return;
        }

        // Some TODO:
        // - The origin_selection_range should be fine here.
        // - THe target_uri is TODO.
        // - The target range is fine...?
        // - The target selection range is TODO.

        let var = db.get(var);

        self.set_link(ast, origin_selection_range,
            &var.location,
            &var.location);
    }
}

fn cursor_on(cursor: &SourceLocation, target: &SourceLocation) -> bool {
    if cursor.offset < target.offset { return false; }
    if cursor.offset > target.offset + target.length { return false; }
    return true;
}

impl<'a> LocateAst for GotoDefinitionVisitor<'a> {
    fn locate_assign(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Assign) {
        // TODO: We could just not even do an origin_selection_range here as the
        // default should be correct...?
        self.goto_var(ast, db, it.identity,  Some(&it.var_name));
    }

    fn locate_variable(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Variable) {
        self.goto_var(ast, db, it.identity, Some(&it.location));
    }

    fn locate_get(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Get) {
        self.goto_var(ast, db, it.var, Some(&it.identifier.location));
    }

    fn locate_set(&mut self, ast: &Ast, db: &Db, _loc: &SourceLocation, it: &Set) {
        self.goto_var(ast, db, it.var, Some(&it.identifier.location));
    }

    fn locate_new(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &New) {
        eprintln!("it.identifier.location: {} ? {} ? {}",
            it.identifier.location.offset,
            loc.offset,
            it.identifier.location.offset + it.identifier.location.length);
        if cursor_on(loc, &it.identifier.location) {
            self.goto_class(ast, db, it.class, Some(&it.identifier.location));
        }
    }
}

// TODO: Support jump-to-definition from whatever document
// We need a good mapping of Url -> SourceId -> Module or something.

pub fn goto_definition(store: &mut DocumentStore, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
    let Some(project) = store.projects.get(&params.text_document_position_params.text_document.uri) else {
        return None;
    };

    let cached = project.get_cache(store);
    let cached = cached.lock().unwrap();

    let Some(id) = cached.url_to_id_map.get(&params.text_document_position_params.text_document.uri) else {
        return None;
    };

    let source_loc = inverse_convert_position(&cached.ast, *id, &params.text_document_position_params.position);

    let mut visitor = GotoDefinitionVisitor {
        response: None,

        id_to_url_map: &cached.id_to_url_map
    };

    visitor.visit_ast(&cached.ast, &cached.db, &source_loc);

    visitor.response
}