use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex, MutexGuard}, time::SystemTime};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use poniescript_core::{
    arena::IndexCell, binder, db::*, init_ordering, module::{self}, source::*, typecheck, Args
};

use crate::{inlay_hint::{compute_inlay_hint_cache, InlayHintCache}, LspArgs};

pub struct Diagnostics {
    pub all: Vec<(Url, Vec<Diagnostic>)>
}

fn parse_all_modules(ast: &mut Ast, db: &mut Db, doc_map: &mut HashMap<SourceId, Arc<Document>>,
    url_to_id_map: &mut HashMap<Url, SourceId>, id_to_url_map: &mut HashMap<SourceId, Url>,
    project: &Project) {

	for doc in &project.files {
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
    pub fn new(document: Arc<Document>) -> Box<dyn SourceProvider + Send + Sync> {
        Box::new(LSPSource { document })
    }
}

impl SourceProvider for LSPSource {
    fn to_reader(&self) -> std::io::Result<Box<dyn std::io::Read>> {
        // Create a clone of the text as a Vec<u8> and wrap it in a cursor
        //  TODO: Can this clone be avoided? Seems very unlikely with the Mutex..
        let text = self.document.text.lock().unwrap();
        let bytes = Box::new(std::io::Cursor::new(text.as_bytes().to_vec()));
        return Ok(bytes);
    }

    fn repr_path(&self) -> String {
        self.document.url.to_string()
    }
}

pub struct Document {
    text: Mutex<String>,
    url: Url,
}

/// The cached Ast and other info for a Project.
pub struct ProjectCache {
    pub db: Db,
    pub ast: Ast,
    pub diagnostics: Option<Diagnostics>,

     /// Maps SourceId's in this Project back to associated Urls.
    pub id_to_url_map: HashMap<SourceId, Url>,
    pub url_to_id_map: HashMap<Url, SourceId>,
}

/// Represents a single Project, as defined in the ponies.toml file. Used
/// to provide support for multiple projects if there are multiple in the
/// workspace.
/// 
/// If there are no ponies.toml files, then we treat each .poni file as its
/// own Project.
pub struct Project {
    // Current design idea: We hold an Arc internally. That way, we can easily
    // safely extract it from the Option. This also means that stuff that is
    // using the old version of the cache will safely ... keep using it ... until
    // it decides to recompute it.
    //
    // TODO: There should be no need to wrap the inner ProjectCache in a Mutex;
    // the only reason we're doing it right now is becaUse we still have the
    // UnsafeCells inside the Arena. We should use an AstReadonly and such...
    cache: Mutex<Option<Arc<Mutex<ProjectCache>>>>,

    files: Vec<Arc<Document>>,
}

impl Project {
    pub fn clear_cache(&self) {
        let mut cache = self.cache.lock().unwrap();
        *cache = None;
    }

    pub fn recompute_cache(self: &Arc<Self>, store: &DocumentStore) -> Arc<Mutex<ProjectCache>> {
        eprintln!("--- re-parse modules ---");
        let start = SystemTime::now();

        let mut ast = Ast::new();
        let mut db = Db::new(&mut ast);

        let mut doc_map: HashMap<SourceId, Arc<Document>> = HashMap::new();
        let mut url_to_id_map = HashMap::new();
        let mut id_to_url_map = HashMap::new();

        parse_all_modules(&mut ast, &mut db, &mut doc_map, &mut url_to_id_map, &mut id_to_url_map, &self);

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
        
        let cache = Arc::new(Mutex::new(ProjectCache {
            db,
            ast,
            diagnostics: Some(err),
            id_to_url_map,
            url_to_id_map,
        }));

        let mut lock = self.cache.lock().unwrap();
        *lock = Some(Arc::clone(&cache));

        cache
    }

    pub fn get_cache(self: &Arc<Self>, store: &DocumentStore) -> Arc<Mutex<ProjectCache>> {
        let lock = self.cache.lock().unwrap();
        if let Some(cache) = lock.as_ref() {
            return Arc::clone(cache);
        }

        drop(lock);

        // Steal the Arc from the recompute function.
        self.recompute_cache(store)
    }
}

pub struct DocumentStore {
    documents: HashMap<Url, Arc<Document>>,

    inlay_hints: HashMap<Url, InlayHintCache>,

    pub projects: HashMap<Url, Arc<Project>>,

    pub c_output: Option<PathBuf>,
}

impl DocumentStore {
    pub fn new(args: &LspArgs) -> Self {
        Self {
            documents: HashMap::new(),
            inlay_hints: HashMap::new(),
            projects: HashMap::new(),
            c_output: args.c_output.clone()
        }
    }

    pub fn update(&mut self, url: &Url, text: String) {
        let doc = self.documents.entry(url.clone())
            .or_insert_with(|| {
                // For now, if we are getting a new Document, also create a new
                // Project with that document.
                let doc = Arc::new(Document { text: Mutex::new(String::new()), url: url.clone() });
            
                let project = Project {
                    cache: Mutex::new(None),
                    files: vec![Arc::clone(&doc)]
                };

                self.projects.insert(url.clone(), Arc::new(project));

                doc
            });

        // Update the text of the given document.
        {
            let mut doc_text = doc.text.lock().unwrap();
            *doc_text = text;
        }

        // Now, invalidate the project for this Url.
        if let Some(project) = self.projects.get(&url) {
            project.clear_cache();
        }

        // Of course, this will need to be made more efficient..
        self.inlay_hints.clear();
    }

    pub fn get_inlay_hint_cache(&mut self, url: &Url) -> Option<&InlayHintCache> {
        if self.inlay_hints.contains_key(url) {
            return Some(self.inlay_hints.get(url).unwrap());
        }

        let Some(project) = self.projects.get(url) else {
            return None;
        };

        let Some(cache) = compute_inlay_hint_cache(self, project, url) else {
            return None;
        };
        self.inlay_hints.insert(url.clone(), cache);

        return Some(self.inlay_hints.get(&url).unwrap())
    }

    pub fn steal_diagnostics(&mut self, url: &Url) -> Option<Diagnostics> {
         let Some(project) = self.projects.get(url) else {
            return None;
        };
        let cache = project.get_cache(self);
        let mut cache = cache.lock().unwrap();

        cache.diagnostics.take()
    }
}