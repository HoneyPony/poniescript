use std::{collections::HashMap, rc::Rc, sync::Arc, time::SystemTime};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use poniescript_core::{
    arena::{ArenaKey, IndexCell}, binder, db::*, expr::*, init_ordering, module::{self, Module}, source::*, typecheck, Args
};

pub struct Diagnostics {
    pub all: Vec<(Url, Vec<Diagnostic>)>
}

fn parse_all_modules(ast: &mut Ast, db: &mut Db, doc_map: &mut HashMap<SourceId, Arc<Document>>,
    url_to_id_map: &mut HashMap<Url, SourceId>, id_to_url_map: &mut HashMap<SourceId, Url>, store: &DocumentStore) -> bool {
	let mut had_error = false;

	for doc in store.documents.values() {
        let source = LSPSource::new(doc.clone());
        let source_id = ast.sources.push(Source::new(source));

        url_to_id_map.insert(doc.url.clone(), source_id);
        id_to_url_map.insert(source_id, doc.url.clone());

        doc_map.insert(source_id, doc.clone());

		match module::parse_module(ast, db, source_id) {
			Ok(_) => { },
			Err(err) => {
                todo!("Report I/O errors to LSP?");
				//eprintln!("Unable to parse source file {}: {err}", path.display());
				had_error = true;
			}
		}
	}

	had_error
}

// TODO: Respect utf-16, utf-8, etc
pub fn convert_position(ast: &Ast, location: &SourceLocation) -> Position {
    let (line, col) = ast.sources.get(location.source).get_line_column(location);
    let (line, col) = (line - 1, col - 1);

    Position { line: line as u32, character: col as u32 }
}

pub fn inverse_convert_position(ast: &Ast, source_id: SourceId, position: &Position) -> SourceLocation {
    let offset = ast.sources.get(source_id).get_offset(position.line as u64, position.character as u64);

    SourceLocation {
        source: source_id,
        offset,
        length: 1,
    }
}

pub fn convert_range(ast: &Ast, location: &SourceLocation) -> Range {
    let end = location.end();
    let start = convert_position(ast, location);
    let end = convert_position(ast, &end);

    Range { start, end }
}

fn report_errors(ast: &Ast, db: &Db, doc_map: &HashMap<SourceId, Arc<Document>>) -> Diagnostics {
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
            range: convert_range(ast, &error.main_location),
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

fn do_handle_files(store: &DocumentStore) -> (Db, Ast, Diagnostics, HashMap<Url, SourceId>, HashMap<SourceId, Url>) {
    eprintln!("--- re-parse modules ---");
    let start = SystemTime::now();
    let mut args = Args::default();
    //args.input_paths.push(path);

    let mut ast = Ast::new();
    let mut db = Db::new(&mut ast);

    let mut doc_map: HashMap<SourceId, Arc<Document>> = HashMap::new();
    let mut url_to_id_map = HashMap::new();
    let mut id_to_url_map = HashMap::new();

    let had_error = parse_all_modules(&mut ast, &mut db, &mut doc_map, &mut url_to_id_map, &mut id_to_url_map, store);

	// if had_error {
    //     let err = report_errors(&db, &doc_map);
    //     return (db, ast, modules, err);
	// }

	// Pass 2: Binding
	let had_error = binder::bind(&mut db, &mut ast);

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
	let had_error = typecheck::typecheck(&mut db, &mut ast);

    let err = report_errors(&ast, &db, &doc_map);
    let end = SystemTime::now();
    eprintln!("rebuild ast/db took {}ms", end.duration_since(start).unwrap().as_millis());
    return (db, ast, err, url_to_id_map, id_to_url_map);
}

pub struct Document {
    text: String,
    url: Url,
}

pub struct DocumentStore {
    documents: HashMap<Url, Arc<Document>>,

    cached_stuff: Option<(Db, Ast, Diagnostics, HashMap<Url, SourceId>, HashMap<SourceId, Url>)>
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
            text, url,
        }));

        self.cached_stuff = None;
    }

    pub fn get_cached_stuff(&mut self) -> &mut (Db, Ast, Diagnostics, HashMap<Url, SourceId>, HashMap<SourceId, Url>) {
        if self.cached_stuff.is_none() {
            self.cached_stuff = Some(do_handle_files(&self));
        }

        return self.cached_stuff.as_mut().unwrap()
    }

    pub fn steal_diagnostics(&mut self) -> Diagnostics {
        self.get_cached_stuff();
        let diagnostics = std::mem::replace(&mut self.cached_stuff.as_mut().unwrap().2, Diagnostics { all: vec![] });
        diagnostics
    }
}