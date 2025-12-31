use std::fs;
use std::path::Path;
use std::path::PathBuf;

use maud::Markup;
use maud::PreEscaped;
use maud::html;
use poniescript_core::db::Ast;
use poniescript_core::db::Db;
use poniescript_core::db::IdFuncs;
use poniescript_core::inf_write;
use poniescript_core::lexer::Token;
use pulldown_cmark::CowStr;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;

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

struct Function {
    name: String,

    // E.g. do_stuff(x: int, y: int) -> void
    repr: String,
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

    /// Member functions for this type, if any.
    member_funs: Vec<Function>,
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
            // TODO: Is there actually any reason to use then_with here?
            // In theory, the parameter names will always be distinct anyway.
            a.name.cmp(&b.name).then_with(|| {
                a.type_repr.cmp(&b.type_repr)
            })
        });

        self.member_funs.sort_by(|a, b| {
            a.name.cmp(&b.name)
        });

        // let parser = pulldown_cmark::Parser::new_ext(&self.main_content_markdown,
        //     pulldown_cmark::Options::ENABLE_HEADING_ATTRIBUTES);
        let parser = pulldown_cmark::Parser::new(&self.main_content_markdown);
        let mut html_output = String::new();

        let mut next_section_id = 0;

        let mut pending_headings = Vec::new();
        // An in-order list of section ID's for the markdown, plus the section
        // name.
        // 
        // These are for headings of level # or ##, I think.
        let mut section_ids = Vec::new();

        let heading_eater = parser.map(|mut evt| {
            match &mut evt {
                pulldown_cmark::Event::Start(tag) => {
                    match tag {
                        Tag::Heading { level, id, classes, attrs } => {
                            if *level <= HeadingLevel::H2 {
                                // Just generate simple numerical section ids for now.
                                //
                                // We could read some from the source markdown, but
                                // I'm not sure there's much reason.
                                //
                                // We may eventually want them to be a bit more
                                // correlated simply so documentation links can
                                // be more stable, but this is fine for now.

                                let new_id = format!("sect-{}", next_section_id);

                                // We have a stack of headings that we push to each
                                // time we see a Heading tag and pop from each time
                                // we close a heading tag.
                                pending_headings.push((new_id.clone(), String::new(), level.clone()));

                                next_section_id += 1;
                                let cow = CowStr::from(new_id);
                                *id = Some(cow);
                            }
                        },
                        _ => {}
                    }
                },
                // pulldown_cmark::Event::SoftBreak => {
                //     if let Some(next) = pending_headings.pop() {
                //         section_ids.push(next);
                //     }
                // }
                pulldown_cmark::Event::End(tag) => {
                    match tag {
                        TagEnd::Heading(level) => {
                            if *level <= HeadingLevel::H2 {
                                // Push the next section ID.
                                section_ids.push(pending_headings.pop().expect("tags should always match"));
                            }
                        }
                        _ => {}
                    }
                }
                pulldown_cmark::Event::Text(text) => {
                    // If we have a current heading, add text to it. This is just
                    // summary text for the sidebar, so we don't care if it's
                    // suuuper great.
                    if let Some(last) = pending_headings.last_mut() {
                        last.1 += text;
                    }
                }
                pulldown_cmark::Event::Code(text) => {
                    // For the summary sidebar, add internal code without
                    // formatting it as code for the sidebar.
                    if let Some(last) = pending_headings.last_mut() {
                        last.1 += text;
                    }
                }
                _ => {}
            }
            evt
        });

        pulldown_cmark::html::push_html(&mut html_output, heading_eater);

        assert!(pending_headings.is_empty());

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
                            @for sect in &section_ids {
                                // TODO: Different h5/h4 depending on which
                                // level of heading it was...?
                                @match sect.2 {
                                    HeadingLevel::H1 => h5 { a href={"#" (sect.0)} { (sect.1) } }
                                    HeadingLevel::H2 => h6 { a href={"#" (sect.0)} { (sect.1) } }
                                    //_ => (unreachable!())
                                    // unreachable() gives a silly warning so
                                    // just generate nothing instead.
                                    _ => {}
                                }
                            }
                            @if self.member_vars.len() > 0 {
                                h5 { a href="#member-vars" { "Member variables" } }
                                @for var in &self.member_vars {
                                    // TODO: Consider making these flash
                                    // or something when you click them?
                                    // In case the thing is already on screen.
                                    h6 { a href={"#var-" (var.name)} { (var.name) } }
                                }
                            }
                            @if self.member_funs.len() > 0 {
                                h5 { a href="#member-funs" { "Member functions" } }
                                @for fun in &self.member_funs {
                                    h6 { a href={"#fun-" (fun.name)} { (fun.name) } }
                                }
                            }
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
                                h1 id="member-vars" { "Member variables" }
                                @for var in &self.member_vars {
                                    h2 id={"var-" (var.name)} {
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

                            // Generate documentation for member functions
                            @if self.member_funs.len() > 0 {
                                h1 id="member-funs" { "Member functions" }
                                @for fun in &self.member_funs {
                                    h2 id={"fun-" (fun.name)} {
                                        code {
                                            span .code-k {
                                                "fun"
                                            }
                                            " "
                                            // TODO: Syntax highlight these.
                                            (fun.repr)
                                        }
                                    }
                                    (convert_member_md(&fun.doc_markdown))
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
        let mut member_funs = Vec::new();
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

        for fun in &class.funs {
            let fun = db.get(*fun);
            
            // To build the string representation:
            // - Start with the fun name
            // - For each variable, add its name and type repr (TODO: typecheck...)
            // - Add the return type

            // Man I'm writing a pretty-printer over here by accident...
            let name = match fun.name {
                Some(name) => db.get(name).to_string(),
                // NOTE: This should (?) be impossible here, but might as well.
                // TBH, we might extract this formatting into some helper
                // function eventually anyway.
                None => "<anonymous>".to_string(),
            };
            let mut repr = format!("{}(", name);
            let mut comma = false;
            for var in &fun.parameters {
                if comma { repr.push_str(", "); }

                let var = db.get(*var);
                // TODO: Use ufmt for everything? :)
                use std::fmt::Write;
                write!(repr, "{}: {}",
                    db.get(var.name), db.repr_type(var.typ)).unwrap();

                comma = true;
            }
            repr.push_str(") -> ");
            repr.push_str(db.repr_type(fun.return_type));

            let fun = Function {
                name,
                repr,
                doc_markdown: convert_doc_comment(&db, &fun.doc_comment),
            };

            member_funs.push(fun);
        }

        let mut doc_page = DocPage {
            section: None,
            identifier: Identifier::class(id),
            main_content_markdown: md,
            static_path: PathBuf::from("../static"),
            member_vars,
            member_funs,
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
            member_funs: vec![],
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