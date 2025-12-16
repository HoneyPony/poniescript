use std::fs;
use std::path::Path;

use maud::Markup;
use maud::PreEscaped;
use maud::html;

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
    ]
}

fn generate_doc_page(identifier: &str, markdown: &str) -> Markup {
    let parser = pulldown_cmark::Parser::new(markdown);
    let mut html_output = String::new();

    pulldown_cmark::html::push_html(&mut html_output, parser);

    html! {
        html {
            head {
                meta charset="utf-8";
                title { (format!("{identifier} | poniescript docs")) }
            }
            body {
                (PreEscaped(html_output))
            }
        }
    }
}

/// Generates docs to the given output path. Note that this will always create
/// the given output path, due to the way it creates the interior paths.
pub fn generate_docs(output_path: &Path) -> std::io::Result<()> {
    let builtins = get_builtins();
    let builtins_dir = output_path.join("builtins");
    fs::create_dir_all(&builtins_dir)?;
    for builtin in builtins {
        let markup = generate_doc_page(builtin.identifier, builtin.markdown);
        fs::write(builtins_dir.join(format!("{}.html", builtin.identifier)),
            markup.0)?;
    }

    Ok(())
}