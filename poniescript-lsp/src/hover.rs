use poniescript_core::lexer::Token;
use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*, inf_write, source::*
};

use crate::document::DocumentStore;
use crate::document::*;
use crate::documentation;
use crate::semantic_locate;
use crate::semantic_locate::Semantic;

struct HoverHelper {
    response: Option<Hover>,
}

fn build_hover(code: &str, doc: Option<String>, range: Option<Range>) -> Hover {
    match doc {
        Some(doc) => {
            Hover {
                contents: HoverContents::Array(
                    vec![
                        MarkedString::LanguageString(LanguageString {
                            language: "poniescript".to_string(),
                            value: code.to_string()
                        }),
                        // Provide a horizontal rule before the documentation.
                        MarkedString::String("---".into()),
                        // This is markdown.
                        MarkedString::String(doc)
                    ]
                ),
                range
            }
        }
        None => {
            Hover {
                contents: HoverContents::Scalar(MarkedString::LanguageString(LanguageString {
                    language: "poniescript".to_string(),
                    value: code.to_string()
                })),
                range
            }
        }
    }
    
}

impl HoverHelper {
    fn build_hover(&mut self, ast: &Ast, title: &str, doc: Option<String>, range: Option<&SourceLocation>) {
        let range = range.map(|r| convert_range(ast, r));
        self.response = Some(build_hover(title, doc, range));
    }

    fn hover_class(&mut self, ast: &Ast, db: &Db, class: ClassId, range: Option<&SourceLocation>) {
        let class = db.get(class);

        let mut class_sig = String::new();
        inf_write!(class_sig, "class {}", db.get(class.name));

        self.build_hover(ast, &class_sig, documentation::inefficient_doc(db, &class.doc_comment), range);
    }

    fn hover_fun(&mut self, ast: &Ast, db: &Db, fun: FunId, range: Option<&SourceLocation>) {
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

        self.build_hover(ast, &fun_sig, documentation::inefficient_doc(db, &fun.doc_comment), range);
    }

    fn hover_var(&mut self, ast: &Ast, db: &Db, var: VarId, range: Option<&SourceLocation>) {
        let var = db.get(var);

        let mut var_sig = String::new();
        inf_write!(var_sig, "var {}: {}", db.get(var.name), db.repr_type(var.typ));

        self.build_hover(ast, &var_sig, documentation::inefficient_doc(db, &var.doc_comment), range);
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

    let mut helper = HoverHelper {
        response: None,
    };

    let db = &cached.db;
    let ast = &cached.ast;

    semantic_locate::semantic_locate(ast, db, source_loc, |semantic, origin_selection_range| {
        match semantic {
            Semantic::Var(var_id) => {
                helper.hover_var(ast, db, var_id, origin_selection_range);
            },
            Semantic::Fun(fun_id) => {
                helper.hover_fun(ast, db, fun_id, origin_selection_range);
            },
            Semantic::Class(class_id) => {
                helper.hover_class(ast, db, class_id, origin_selection_range);
            },
            Semantic::Print => {
                helper.build_hover(ast, "print(args: ...) -> <first>",
Some("Prints out any series of expressions. Each expression is printed to the console
in order. The print will be terminated by a newline.

`print()` does not add any whitespace besides the terminating newline. If you wish
to separate arguments, you should intersperse them manually. For example:
```
class Horse { name: StrBuf = \"Twilight\"; age: int = 30; }
var horse = new Horse{};
print(horse.name, \" \", horse.age); // prints \"Twilight 30\"
```

If you wish to convert a series of expressions into a `StrBuf`, use `str()` instead.

### Returns
print() always returns the result of its first argument. This allows you to
intersperse print() with existing logic, such as:
```poniescript
if print(a > b) {
    do_high_a_logic();
}
```".into()), origin_selection_range)
            }
        }
    });

    helper.response
}