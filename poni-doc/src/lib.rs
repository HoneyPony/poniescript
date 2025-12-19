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
    identifier: Identifier,
    markdown: &'static str,
}

impl BuiltinDoc {
    fn new(identifier: &'static str, kind: IdKind, markdown: &'static str) -> Self {
        Self {
            identifier: Identifier { id: identifier.to_string(), kind },
            markdown,
        }
    }
}

fn get_builtins() -> Vec<BuiltinDoc> {
    vec![
        BuiltinDoc::new("print", IdKind::Function, include_str!("builtins/print.md")),
        BuiltinDoc::new("str", IdKind::Function, include_str!("builtins/str.md")),
        BuiltinDoc::new("DynArray", IdKind::Class, include_str!("builtins/DynArray.md")),
    ]
}

enum IdKind {
    Function,
    Class,
    Variable
}

impl IdKind {
    pub fn repr_keyword(&self) -> &'static str {
        match self {
            IdKind::Function => "fun",
            IdKind::Class => "class",
            IdKind::Variable => "var",
        }
    }
}

struct Identifier {
    id: String,
    kind: IdKind,
}

impl Identifier {
    pub fn class<S: ToString>(s: S) -> Self {
        Identifier { id: s.to_string(), kind: IdKind::Class }
    }
}

// struct MemberList {
//     list_name: String,
//     // List of tuples of (identifier, member documentation).
//     members: Vec<(Identifier, String)>,
// }

struct Variable {
    name: String,
    type_repr: String,
    doc_markdown: String,
}

struct DocPage {
    /// What section the doc page is in, e.g. 'PonieScript Builtins'
    section: Option<String>,

    /// The core identifier for the doc page
    identifier: Identifier,
    
    /// Markdown representing the main content of the page
    main_content_markdown: String,

    /// The Path to the 'static' content for the site, for pulling resources
    /// such as the css files.
    static_path: PathBuf,

    /// Member variables for this type, if any.
    member_vars: Vec<Variable>,
}

// TODO: We will probably want to delete / exchange headers somehow in these?
fn convert_member_md(markdown: &String) -> PreEscaped<String> {
    // Fast path for empty strings.
    if markdown.is_empty() {
        return PreEscaped(String::new());
    }

    let parser = pulldown_cmark::Parser::new(&markdown);
    let mut html = String::new();

    pulldown_cmark::html::push_html(&mut html, parser);
    PreEscaped(html)
}

impl DocPage {
    pub fn generate_html(&mut self) -> Markup {
        // Alphabatize members.
        self.member_vars.sort_by(|a, b| {
            a.name.cmp(&b.name).then_with(|| {
                a.type_repr.cmp(&b.type_repr)
            })
        });

        let parser = pulldown_cmark::Parser::new(&self.main_content_markdown);
        let mut html_output = String::new();

        pulldown_cmark::html::push_html(&mut html_output, parser);

        html! {
            html {
                head {
                    meta charset="utf-8";
                    // TODO: Use utf-8 paths here as we need to generate
                    // valid paths even on Windows
                    link rel="stylesheet" href=(self.static_path.join("main.css").display());
                    link rel="stylesheet" href=(self.static_path.join("syntax.css").display());
                    title { (format!("{} | poniescript docs", self.identifier.id)) }
                }
                body {
                    .poni-doc {
                        nav .poni-doc-nav {
                            @match &self.section {
                                Some(h) => h4 { (h) }
                                None => {}
                            }
                            h4 { (self.identifier.id) }
                            @if self.member_vars.len() > 0 {
                                h5 { "Member variables" }
                                @for var in &self.member_vars {
                                    // TODO: actually link these up
                                    a { (var.name) }
                                }
                            }
                            h5 { "Member functions" }
                        }
                        .poni-doc-content {
                            // Create a heading that is based on the identifier
                            // kind
                            h1 {
                                code {
                                    span .code-k {
                                        (self.identifier.kind.repr_keyword())
                                    }
                                    // Explicit space
                                    " "
                                    (self.identifier.id)
                                }
                            }
                            (PreEscaped(html_output))

                            // Generate documentation for member variables
                            @if self.member_vars.len() > 0 {
                                // NOTE: For jumping-to-section, we will have
                                // to prepend something to user-defined sections
                                // so they don't interfere with our own.
                                h1 { "Member variables" }
                                @for var in &self.member_vars {
                                    h2 {
                                        code {
                                            span .code-k {
                                                "var"
                                            }
                                            " "
                                            (var.name)
                                            ": "
                                            // TODO: Syntax highlight these too.
                                            (var.type_repr)
                                        }
                                    }
                                    (convert_member_md(&var.doc_markdown))
                                }
                            }
                        }
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

            // There is no need to terminate the lines with a '\n', as the
            // newline will be included from the source.
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

        let mut member_vars = Vec::new();
        for var in &class.vars {
            let var = db.get(*var);
            let name = db.get(var.name);
            // We are going to have to run type checking...?
            // :(
            let typ = db.repr_type(var.typ);

            let var = Variable {
                name: name.to_string(),
                // TODO: Reduce number of to_string()'s here? There might be
                // a way to just have stuff pointing into the Db.
                type_repr: typ.to_string(),
                doc_markdown: convert_doc_comment(&db, &var.doc_comment),
            };

            member_vars.push(var);
        }

        let mut doc_page = DocPage {
            section: None,
            identifier: Identifier::class(id),
            main_content_markdown: md,
            static_path: PathBuf::from("../static"),
            member_vars,
        };

        let markup = doc_page.generate_html();
        fs::write(generated.join(format!("{}.html", id)), markup.0)?;
    }

    let builtins = get_builtins();
    let builtins_dir = output_path.join("builtins");
    fs::create_dir_all(&builtins_dir)?;
    for builtin in builtins {
        let output_path = builtins_dir.join(format!("{}.html", builtin.identifier.id));

        let mut doc_page = DocPage {
            section: None,
            identifier: builtin.identifier,
            main_content_markdown: builtin.markdown.to_string(),
            static_path: PathBuf::from("../static"),
            member_vars: vec![],
        };

        let markup = doc_page.generate_html();
        fs::write(output_path, markup.0)?;
    }

    // Copy static files.
    let statics_dir = output_path.join("static");
    fs::create_dir_all(&statics_dir)?;
    fs::write(statics_dir.join("main.css"),
        include_bytes!("static/main.css"))?;
    fs::write(statics_dir.join("syntax.css"),
        include_bytes!("static/syntax.css"))?;

    Ok(())
}