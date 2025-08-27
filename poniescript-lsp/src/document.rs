use std::{collections::HashMap, rc::Rc, sync::Arc};

use tower_lsp::lsp_types::Url;

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

fn parse_all_modules(ast: &mut Ast, db: &mut Db, store: &DocumentStore) -> (Vec<Module>, bool) {
	let mut modules = vec![];

	let mut had_error = false;

	for doc in store.documents.values() {
        let source = LSPSource::new(doc.clone());
        let source_id = db.put_source_provider(source);
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

fn report_errors(db: &Db) {
    eprintln!("Errors discovered in source code");
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

fn do_handle_files(store: &DocumentStore) -> (Db, Ast, Vec<Module>) {
    let mut args = Args::default();
    //args.input_paths.push(path);

    let mut db = Db::new();
    let mut ast = Ast::new();

    let (mut modules, had_error) = parse_all_modules(&mut ast, &mut db, store);

	if had_error {
		report_errors(&db);
        return (db, ast, modules);
	}

	// Pass 2: Binding
	let had_error = binder::bind(&mut db, &mut ast, &mut modules);

	if had_error {
		report_errors(&db);
		return (db, ast, modules);
	}

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

	if !db.errors.is_empty() {
		report_errors(&db);
		return (db, ast, modules);
	}

	// Pass 4: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut ast, &mut modules);

	if had_error {
		report_errors(&db);
	}

    return (db, ast, modules);
}

pub struct Document {
    text: String,
    url: Url
}

pub struct DocumentStore {
    documents: HashMap<Url, Arc<Document>>,

    cached_stuff: Option<(Db, Ast, Vec<Module>)>
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

    pub fn get_cached_stuff(&mut self) -> &mut (Db, Ast, Vec<Module>) {
        if self.cached_stuff.is_none() {
            self.cached_stuff = Some(do_handle_files(&self));
        }

        return self.cached_stuff.as_mut().unwrap()
    }
}