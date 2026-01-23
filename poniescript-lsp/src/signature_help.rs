use tower_lsp::lsp_types::{ParameterInformation, ParameterLabel, SignatureHelp, SignatureHelpParams, SignatureInformation};

use poniescript_core::{
    db::*, expr::*, source::SourceLocation
};
use poni_arena::IndexCell;

use std::sync::Arc;

use crate::{document::{DocumentStore, inverse_convert_position}, documentation};

struct SignatureHelpVisitor {
    result: Option<SignatureHelp>,
}

impl SignatureHelpVisitor {
    fn found_fun(&mut self, db: &Db, fun: FunId, param: u32) {
        let fun = db.get(fun);

        let mut parameters = Vec::new();
        let mut signature = String::new();
        signature.push_str("fun ");
        signature.push_str(db.get(fun.name.unwrap_or(db.str_lambda)));
        signature.push_str("(");

        let mut comma = false;
        for param in &fun.parameters {
            let var = db.get(*param);

            if comma { signature.push_str(", "); }
            comma = true;

            let start_idx = signature.len();
            signature.push_str(db.get(var.name));
            let end_idx = signature.len();
            
            parameters.push(ParameterInformation {
                label: ParameterLabel::LabelOffsets([start_idx as u32, end_idx as u32]),
                documentation: documentation::inefficient_doc_lsp(db, &var.doc_comment),
            });
        }

        signature.push_str(")");

        self.result = Some(SignatureHelp {
            signatures: vec![
                SignatureInformation {
                    label: signature,
                    documentation: documentation::inefficient_doc_lsp(db, &fun.doc_comment),
                    parameters: Some(parameters),
                    active_parameter: None
                }
            ],
            // No function overloading right now.
            active_signature: Some(0),
            active_parameter: Some(param),
        })
    }
}

impl poniescript_core::expr::LocateAst for SignatureHelpVisitor {
    fn locate_funcall(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &FunCall) {
        let fun = db.get(it.identity);

        // Default to last parameter ..?
        let mut param = (fun.parameters.len() - 1) as u32;

        let offset = (loc.offset - it.location.offset) as u32;
        for i in 0..it.arg_boundaries.len() - 1 {
            if offset >= it.arg_boundaries[i] && offset <= it.arg_boundaries[i + 1] {
                param = i as u32;
            }
        }

        // WIP: Pretend that we are always on the first parameter.
        self.found_fun(db, it.identity, param);
    }
}

pub fn signature_help(store: &mut DocumentStore, params: SignatureHelpParams) -> Option<SignatureHelp> {
    let Some(project) = store.projects.get(&params.text_document_position_params.text_document.uri) else {
        return None;
    };

    let cached = project.get_cache(store);
    let cached = cached.lock().unwrap();

    let Some(id) = cached.url_to_id_map.get(&params.text_document_position_params.text_document.uri) else {
        return None;
    };

    let source_loc = inverse_convert_position(&cached.ast, *id, &params.text_document_position_params.position);

    let mut visitor = SignatureHelpVisitor { result: None, };

    visitor.visit_ast(&cached.ast, &cached.db, &source_loc);
    visitor.result
}