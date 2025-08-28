use std::{collections::HashMap, rc::Rc, sync::Arc};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use poniescript_core::{
    db::*,
    expr::*,

    binder,
    module,
    module::Module,
    init_ordering,
    typecheck,

    source::*,

    Args
};

pub struct Diagnostics {
    pub all: Vec<(Url, Vec<Diagnostic>)>
}

fn parse_all_modules(ast: &mut Ast, db: &mut Db, doc_map: &mut HashMap<SourceId, Arc<Document>>, store: &DocumentStore) -> (Vec<Module>, bool) {
	let mut modules = vec![];

	let mut had_error = false;

	for doc in store.documents.values() {
        let source = LSPSource::new(doc.clone());
        let source_id = db.put_source_provider(source);

        doc_map.insert(source_id, doc.clone());

		match module::parse_module(ast, db, source_id) {
			Ok((module, false)) => { modules.push(module) },
			Ok((_, true)) => {
				had_error = true;
			}
			Err(err) => {
				//eprintln!("Unable to parse source file {}: {err}", path.display());
				had_error = true;
			}
		}
	}

	(modules, had_error)
}

// TODO: Respect utf-16, utf-8, etc
fn convert_position(db: &Db, location: &SourceLocation) -> Position {
    let (line, col) = db.get(location.source).get_line_column(location);
    let (line, col) = (line - 1, col - 1);

    Position { line: line as u32, character: col as u32 }
}

fn convert_range(db: &Db, location: &SourceLocation) -> Range {
    let end = location.end();
    let start = convert_position(db, location);
    let end = convert_position(db, &end);

    Range { start, end }
}

fn report_errors(db: &Db, doc_map: &HashMap<SourceId, Arc<Document>>) -> Diagnostics {
    let mut diags = Diagnostics { all: vec![] };

    let mut idx_map: HashMap<SourceId, usize> = HashMap::new();
    for (k, v) in doc_map {
        // We must create an empty set of diagnostics for each source, in the
        // case that there are no more errors.
        diags.all.push((v.url.clone(), vec![]));
        
        idx_map.insert(*k, diags.all.len() - 1);
    }

    for error in &db.errors {
        let diag = Diagnostic {
            range: convert_range(db, &error.main_location),
            severity: Some(if error.is_warning { DiagnosticSeverity::WARNING } else { DiagnosticSeverity::ERROR }),
            code: None,
            code_description: None,
            source: Some("poniescript".into()),
            message: error.main_message.clone(),
            related_information: None,
            tags: None,
            data: None,
        };

        // Safety: We should have pushed an idx for every SourceId.
        let idx = *idx_map.get(&error.main_location.source).unwrap();
        diags.all[idx].1.push(diag);
	}

    return diags;
}

struct LSPSource {
    document: Arc<Document>,
}

impl LSPSource {
    pub fn new(document: Arc<Document>) -> Box<dyn SourceProvider + Send> {
        Box::new(LSPSource { document })
    }
}

impl SourceProvider for LSPSource {
    fn to_reader(&self) -> std::io::Result<Box<dyn std::io::Read>> {
        // Create a clone of the text as a Vec<u8> and wrap it in a cursor
        //  TODO: Can this clone be avoided?
        let bytes = Box::new(std::io::Cursor::new(self.document.text.as_bytes().to_vec()));
        return Ok(bytes);
    }
}

fn do_handle_files(store: &DocumentStore) -> (Db, Ast, Vec<Module>, Diagnostics) {
    let mut args = Args::default();
    //args.input_paths.push(path);

    let mut db = Db::new();
    let mut ast = Ast::new();

    let mut doc_map: HashMap<SourceId, Arc<Document>> = HashMap::new();

    let (mut modules, had_error) = parse_all_modules(&mut ast, &mut db, &mut doc_map, store);

	// if had_error {
    //     let err = report_errors(&db, &doc_map);
    //     return (db, ast, modules, err);
	// }

	// Pass 2: Binding
	let had_error = binder::bind(&mut db, &mut ast, &mut modules);

	// if had_error {
	// 	let err = report_errors(&db, &doc_map);
    //     return (db, ast, modules, err);
	// }

	// Pass 3: Initialization orders. Fix initialization order of various things,
	// including globals.
	//
	// This must come before type check, otherwise the type checker won't be
	// able to figure out the types of certain global patterns (e.g. cyclic/globals_same)
	//
	// It is also valid for it to come after binding, as after that, all the variables
	// are essentially lexically bound, and we don't actually care about type
	// information during the sorting stage.
	// TODO: Is there a way to make this pattern cleaner..?
	let mut globals = std::mem::take(&mut db.globals);
	init_ordering::topological_sort(&mut globals, &ast, &mut db);

	for class in db.iter_class() {
		let mut vars = std::mem::take(&mut db.get_mut(class).vars);

		init_ordering::topological_sort(&mut vars, &ast, &mut db);

		db.get_mut(class).vars = vars;
	}
	db.globals = globals;

	// if !db.errors.is_empty() {
	//     let err = report_errors(&db, &doc_map);
    //     return (db, ast, modules, err);
	// }

	// Pass 4: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut ast, &mut modules);

    let err = report_errors(&db, &doc_map);
    return (db, ast, modules, err);
}

pub struct Document {
    text: String,
    url: Url
}

pub struct DocumentStore {
    documents: HashMap<Url, Arc<Document>>,

    cached_stuff: Option<(Db, Ast, Vec<Module>, Diagnostics)>
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            cached_stuff: None
        }
    }

    pub fn update(&mut self, url: Url, text: String) {
        self.documents.insert(url.clone(), Arc::new(Document {
            text, url
        }));

        self.cached_stuff = None;
    }

    pub fn get_cached_stuff(&mut self) -> &mut (Db, Ast, Vec<Module>, Diagnostics) {
        if self.cached_stuff.is_none() {
            self.cached_stuff = Some(do_handle_files(&self));
        }

        return self.cached_stuff.as_mut().unwrap()
    }

    pub fn steal_diagnostics(&mut self) -> Diagnostics {
        self.get_cached_stuff();
        let diagnostics = std::mem::replace(&mut self.cached_stuff.as_mut().unwrap().3, Diagnostics { all: vec![] });
        diagnostics
    }
}