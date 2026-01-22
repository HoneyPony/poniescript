use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*,
    expr::*,
    source::*,
};

use crate::document::DocumentStore;
use crate::semantic_locate::Semantic;
use crate::{document::*, semantic_locate};

struct GotoDefinitionHelper<'map> {
    response: Option<GotoDefinitionResponse>,
    id_to_url_map: &'map HashMap<SourceId, Url>
}

impl<'map> GotoDefinitionHelper<'map> {
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
        let class = db.get(class);

        // Target selection range TODO.

        self.set_link(ast, origin_selection_range,
            &class.location,
            &class.location);
    }

    fn goto_fun(&mut self, ast: &Ast, db: &Db, fun: FunId, origin_selection_range: Option<&SourceLocation>) {
        let fun = db.get(fun);

        self.set_link(ast, origin_selection_range,
            &fun.location,
            &fun.location);
    }

    fn goto_var(&mut self, ast: &Ast, db: &Db, var: VarId, origin_selection_range: Option<&SourceLocation>) {
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

    let mut helper = GotoDefinitionHelper {
        response: None,
        id_to_url_map: &cached.id_to_url_map
    };

    let ast = &cached.ast;
    let db = &cached.db;

    semantic_locate::semantic_locate(ast, db, source_loc, |semantic, origin_selection_range| {
        match semantic {
            Semantic::Var(var_id) => {
                helper.goto_var(ast, db, var_id, origin_selection_range);
            },
            Semantic::Fun(fun_id) => {
                helper.goto_fun(ast, db, fun_id, origin_selection_range);
            },
            Semantic::Class(class_id) => {
                helper.goto_class(ast, db, class_id, origin_selection_range);
            },
        }
    });
    
    helper.response
}