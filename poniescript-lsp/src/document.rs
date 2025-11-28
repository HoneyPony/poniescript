use std::{collections::HashMap, path::PathBuf, sync::Arc, time::SystemTime};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use poniescript_core::{
    arena::IndexCell, binder, db::*, init_ordering, module::{self}, source::*, typecheck, Args
};

use crate::{inlay_hint::{compute_inlay_hint_cache, InlayHintCache}, LspArgs};

pub struct Diagnostics {
    pub all: Vec<(Url, Vec<Diagnostic>)>
}

fn parse_all_modules(ast: &mut Ast, db: &mut Db, doc_map: &mut HashMap<SourceId, Arc<Document>>,
    url_to_id_map: &mut HashMap<Url, SourceId>, id_to_url_map: &mut HashMap<SourceId, Url>, store: &DocumentStore) {

	for doc in store.documents.values() {
        let source = LSPSource::new(doc.clone());
        let source_id = ast.sources.push(Source::new(source));

        url_to_id_map.insert(doc.url.clone(), source_id);
        id_to_url_map.insert(source_id, doc.url.clone());

        doc_map.insert(source_id, doc.clone());

		match module::parse_module(ast, db, source_id) {
			Ok(_) => { },
			Err(_) => {
                todo!("Report I/O errors to LSP?");
			}
		}
	}
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

    fn repr_path(&self) -> String {
        self.document.url.to_string()
    }
}

// fn do_finish_compile(db: &mut Db, mut ast: Ast, output: &PathBuf, args: &Args) -> Ast {
//     poniescript_core::dead_code::eliminate_dead_code(db, &mut ast);

//     // Sort value types.
//     if db.sort_value_types().is_err() {
//         // TODO: Report these errors
//         return ast;
//     }

//     // Pass 5: Codegen
//     // Generate any caches that require type checking info.
//     db.generate_codegen_caches(&args);

//     // TODO: Figure out how to make this async correctly...?
//     let Ok(mut output) = std::fs::File::create(output) else {
//         eprintln!("Warning: Unable to create output C file");
//         return ast;
//     };

//     let (ast, sources) = ast.into_readonly();
//     let ast = Arc::new(ast);

//     if let Err(err) = poniescript_core::codegen::codegen(&args, db, Arc::clone(&ast), &mut output) {
//         eprintln!("Warning: Unable to write C file: {err}");
//     }

//     let Ok(ast) = Arc::try_unwrap(ast) else { unreachable!() };
//     Ast::from_readonly(ast, sources)
// }

fn do_handle_files(store: &DocumentStore) -> (Db, Ast, Diagnostics, HashMap<Url, SourceId>, HashMap<SourceId, Url>) {
    eprintln!("--- re-parse modules ---");
    let start = SystemTime::now();

    let mut ast = Ast::new();
    let mut db = Db::new(&mut ast);

    let mut doc_map: HashMap<SourceId, Arc<Document>> = HashMap::new();
    let mut url_to_id_map = HashMap::new();
    let mut id_to_url_map = HashMap::new();

    parse_all_modules(&mut ast, &mut db, &mut doc_map, &mut url_to_id_map, &mut id_to_url_map, store);

	// Pass 2: Binding
	binder::bind(&mut db, &mut ast);


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

	// Pass 4: Type check and infer
	typecheck::typecheck(&mut db, &mut ast);

    let err = report_errors(&ast, &db, &doc_map);
    let end = SystemTime::now();
    eprintln!("rebuild ast/db took {}ms", end.duration_since(start).unwrap().as_millis());

    if db.errors.len() == 0 {
        let mut args = Args::default();
        // For now, these are the only args that the LSP is doing. At some point,
        // we probably want to make the Args available as part of core?
        args.hot = true;
        args.engine = true;

        if let Some(output) = &store.c_output {
            //ast = do_finish_compile(&mut db, ast, output, &args);
        }
    }
    
    return (db, ast, err, url_to_id_map, id_to_url_map);
}

pub struct Document {
    text: String,
    url: Url,
}

pub struct DocumentStore {
    documents: HashMap<Url, Arc<Document>>,

    inlay_hints: HashMap<Url, InlayHintCache>,

    cached_stuff: Option<(Db, Ast, Diagnostics, HashMap<Url, SourceId>, HashMap<SourceId, Url>)>,

    pub c_output: Option<PathBuf>,
}

impl DocumentStore {
    pub fn new(args: &LspArgs) -> Self {
        Self {
            documents: HashMap::new(),
            inlay_hints: HashMap::new(),
            cached_stuff: None,
            c_output: args.c_output.clone()
        }
    }

    pub fn update(&mut self, url: Url, text: String) {
        self.documents.insert(url.clone(), Arc::new(Document {
            text, url,
        }));

        self.cached_stuff = None;
        // Of course, this will need to be made more efficient..
        self.inlay_hints.clear();
    }

    pub fn get_cached_stuff(&mut self) -> &mut (Db, Ast, Diagnostics, HashMap<Url, SourceId>, HashMap<SourceId, Url>) {
        if self.cached_stuff.is_none() {
            self.cached_stuff = Some(do_handle_files(&self));
        }

        return self.cached_stuff.as_mut().unwrap()
    }

    pub fn get_inlay_hint_cache(&mut self, url: &Url) -> Option<&InlayHintCache> {
        if self.inlay_hints.contains_key(url) {
            return Some(self.inlay_hints.get(url).unwrap());
        }

        let Some(id) = self.get_cached_stuff().3.get(url).copied() else {
            return None;
        };

        let cache = compute_inlay_hint_cache(id, self);
        self.inlay_hints.insert(url.clone(), cache);

        return Some(self.inlay_hints.get(&url).unwrap())
    }

    pub fn steal_diagnostics(&mut self) -> Diagnostics {
        self.get_cached_stuff();
        let diagnostics = std::mem::replace(&mut self.cached_stuff.as_mut().unwrap().2, Diagnostics { all: vec![] });
        diagnostics
    }
}