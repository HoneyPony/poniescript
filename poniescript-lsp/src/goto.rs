use poniescript_core::arena::ArenaKey;
use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*,
    expr::*,
    source::*,
};

use crate::document::DocumentStore;
use crate::document::*;

struct GotoDefinitionVisitor {
    response: Option<GotoDefinitionResponse>,

    todo_uri_remove_this: Url,
}

impl LocateAst for GotoDefinitionVisitor {
    fn locate_variable(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &Variable) {
        if it.identity == db.var_unassigned {
            // We don't know what it is.
            return;
        }

        let var = db.get(it.identity);

        // Some TODO:
        // - The origin_selection_range should be fine here.
        // - THe target_uri is TODO.
        // - The target range is fine...?
        // - The target selection range is TODO.

        let origin_selection_range = Some(convert_range(db, loc));
        let target_range = convert_range(db, &var.location);
        let target_selection_range = convert_range(db, &var.location);

        let link = LocationLink {
            origin_selection_range,
            target_uri: self.todo_uri_remove_this.clone(),
            target_range,
            target_selection_range
        };
        let response = GotoDefinitionResponse::Link(vec![link]);

        self.response = Some(response);
    }
}

// TODO: Support jump-to-definition from whatever document
// We need a good mapping of Url -> SourceId -> Module or something.

pub fn goto_definition(store: &mut DocumentStore, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
    let (db, ast, modules, _) = store.get_cached_stuff();

    let mut visitor = GotoDefinitionVisitor {
        response: None,

        // For now we just assume the thing is inside the same document
        todo_uri_remove_this: params.text_document_position_params.text_document.uri.clone(),
    };

    let todo_source_id_remove_this = unsafe { SourceId::from_nonzero_u32(std::num::NonZeroU32::new_unchecked(2)) };
    let loc = inverse_convert_position(db, todo_source_id_remove_this, &params.text_document_position_params.position);

    // Also for now we just assume the thing is in Module 0
    if let Some(m) = modules.get(0) {
        for fun in &m.functions {
            visitor.visit_expr(ast, db, &loc, fun.value);
        }
    }

    visitor.response
}