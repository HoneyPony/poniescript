use std::fs;
use std::path::Path;
use std::path::PathBuf;

use maud::Markup;
use maud::PreEscaped;
use maud::html;
use poniescript_core::db::Ast;
use poniescript_core::db::Db;
use poniescript_core::db::IdFuncs;
use poniescript_core::lexer::Token;

struct BuiltinDoc {
    identifier: &'static str,
    markdown: &'static str,
}

impl BuiltinDoc {
    fn new(identifier: &'static str, markdown: &'static str) -> Self {
        Self {
            identifier,
            markdown,
        }
    }
}

fn get_builtins() -> Vec<BuiltinDoc> {
    vec![
        BuiltinDoc::new("print", include_str!("builtins/print.md")),
        BuiltinDoc::new("str", include_str!("builtins/str.md")),
        BuiltinDoc::new("DynArray", include_str!("builtins/DynArray.md")),
    ]
}

fn generate_doc_page(heading: Option<&str>, identifier: &str, markdown: &str, static_path: &Path) -> Markup {
    let parser = pulldown_cmark::Parser::new(markdown);
    let mut html_output = String::new();

    pulldown_cmark::html::push_html(&mut html_output, parser);

    html! {
        html {
            head {
                meta charset="utf-8";
                link rel="stylesheet" href=(static_path.join("main.css").display());
                title { (format!("{identifier} | poniescript docs")) }
            }
            body {
                .poni-doc {
                    nav .poni-doc-nav {
                        @match heading {
                            Some(h) => h4 { (h) }
                            None => {}
                        }
                        h4 { (identifier) }
                        h5 { "Member variables" }
                        h5 { "Member functions" }
                    }
                    .poni-doc-content {
                        (PreEscaped(html_output))
                    }
                }
            }
        }
    }
}

fn parse_modules(ast: &mut Ast, db: &mut Db, input_paths: &Vec<PathBuf>) {
	for path in input_paths {
		let source_id = ast.new_source(path.clone());
		match poniescript_core::module::parse_module(ast, db, source_id) {
			Ok(false) => { },
			Ok(true) => {
                // TODO: Return a Result instead of exiting the process. :(
				std::process::exit(1);
			}
			Err(err) => {
				eprintln!("Unable to parse source file {}: {err}", path.display());
				std::process::exit(1);
			}
		}
	}
}

fn convert_doc_comment(db: &Db, doc_comment: &Option<Vec<Token>>) -> String {
    let mut markdown = String::new();

    if let Some(doc) = doc_comment {
        for tok in doc {
            markdown.push_str(db.get(tok.lexeme));
            //markdown.push('\n');
        }
    }

    return markdown;
}

/// Generates docs to the given output path. Note that this will always create
/// the given output path, due to the way it creates the interior paths.
pub fn generate_docs(input_paths: &Vec<PathBuf>, output_path: &Path) -> std::io::Result<()> {
    let mut ast = Ast::new();
    let mut db = Db::new(&mut ast);

    parse_modules(&mut ast, &mut db, input_paths);

    // TODO: How to distribute directories for these files?
    let generated = output_path.join("generated");
    fs::create_dir_all(&generated)?;
    for class in db.iter_class() {
        // We generate documentation for each class irrespective of whether
        // it has a doc comment.
        // We don't have a tons of structure for this yet; but it should be
        // decent eventually.
        
        let class = db.get(class);
        let md = convert_doc_comment(&db, &class.doc_comment);

        let id: &&str = db.get(class.name);
        let markup = generate_doc_page(None, id, &md,
            Path::new("../static"));
        
        fs::write(generated.join(format!("{}.html", id)), markup.0)?;
    }

    let builtins = get_builtins();
    let builtins_dir = output_path.join("builtins");
    fs::create_dir_all(&builtins_dir)?;
    for builtin in builtins {
        let markup = generate_doc_page(
            Some("PonieScript builtins"),
            builtin.identifier,
            builtin.markdown,
            Path::new("../static"));
        fs::write(builtins_dir.join(format!("{}.html", builtin.identifier)),
            markup.0)?;
    }

    // Copy static files.
    let statics_dir = output_path.join("static");
    fs::create_dir_all(&statics_dir)?;
    fs::write(statics_dir.join("main.css"),
        include_bytes!("static/main.css"))?;

    Ok(())
}