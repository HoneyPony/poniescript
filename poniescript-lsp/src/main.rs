mod document;

use std::path::PathBuf;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

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

use crate::document::*;

fn parse_all_modules(ast: &mut Ast, db: &mut Db, args: &Args) -> (Vec<Module>, bool) {
	let mut modules = vec![];

	let mut had_error = false;

	for path in &args.input_paths {
		match module::parse_module(ast, db, &path) {
			Ok((module, false)) => { modules.push(module) },
			Ok((_, true)) => {
				had_error = true;
			}
			Err(err) => {
				eprintln!("Unable to parse source file {}: {err}", path.display());
				had_error = true;
			}
		}
	}

	(modules, had_error)
}

fn report_errors(db: &Db) {
    eprintln!("Errors discovered in source code");
}

fn do_handle_files(path: PathBuf) -> (Db, Ast, Vec<Module>) {
    let mut args = Args::default();
    args.input_paths.push(path);

    let mut db = Db::new();
    let mut ast = Ast::new();

    let (mut modules, had_error) = parse_all_modules(&mut ast, &mut db, &args);

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

struct SemanticTokenVisitor {
    tokens: Vec<SemanticToken>,
    cursor_start: u64,
    cursor_line: u64,
}

impl SemanticTokenVisitor {
    fn push_token(&mut self, db: &Db, location: &SourceLocation, token_type: u32, token_modifiers_bitset: u32) {
        let (line, col) = db.get(location.source).get_line_column(location);
        let (line, col) = (line - 1, col - 1);

        let mut delta_line: u32 = 0;
        let delta_start: u32;

        if line == self.cursor_line {
            delta_start = (col - self.cursor_start) as u32;
        }
        else {
            delta_line = (line - self.cursor_line) as u32;
            delta_start = col as u32;
        }

        self.cursor_line = line;
        self.cursor_start = col;

        eprintln!("{}:{}: length: {}", self.cursor_line, self.cursor_start, location.length);

        self.tokens.push(SemanticToken { delta_line, delta_start, length: location.length as u32, token_type, token_modifiers_bitset });
    }

    fn push_var(&mut self, db: &Db, location: &SourceLocation, id: VarId) {
        let is_param = db.get(id).fun.is_some();

        self.push_token(db, location, if is_param { 1 } else { 0 }, 0);
    }

    fn push_fun(&mut self, db: &Db, location: &SourceLocation) {
        eprintln!("push fun: {}", location.length);
        self.push_token(db, location, 2, 0);
    }
}

// TODO: Deduplicate this
macro_rules! into {
    ($value:expr, $variant:ident) => {
        {
            let Expr::$variant(v) = $value else { unreachable!() };
            v
        }
    };
}

// TODO: Consider making VisitAst visit each node strongly-typed or something..?
impl poniescript_core::expr::VisitAst for SemanticTokenVisitor {
    fn visit_assign(&mut self, ast: &Ast, db: &mut Db, id: ExprId) {
        // TODO: Visit nested
        let binding = ast.get_expr(id);
        let assign = into!(binding.as_ref(), Assign);

        self.push_var(db, &assign.location, assign.identity);
        self.visit_expr(ast, db, assign.value);
    }

    fn visit_variable(&mut self,ast: &Ast, db: &mut Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let var = into!(binding.as_ref(), Variable);

        self.push_var(db, &var.location, var.identity);
    }

    fn visit_funcapture(&mut self,ast: &Ast, db: &mut Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let capt = into!(binding.as_ref(), FunCapture);

        self.push_fun(db, &capt.fn_name);
    }

    fn visit_funcall(&mut self,ast: &Ast, db: &mut Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let call = into!(binding.as_ref(), FunCall);

        self.push_fun(db, &call.fn_name);

        for arg in &call.args {
            self.visit_expr(ast, db, *arg);
        }
    }
}

struct Backend {
    client: Client,
    store: RwLock<DocumentStore>,
}

fn build_hover(title: &str, contents: &str) -> Hover {
    Hover {
        contents: HoverContents::Array(
            vec![
                MarkedString::LanguageString(LanguageString {
                    language: "poniescript".to_string(),
                    value: title.to_string()
                }),
                MarkedString::String(contents.to_string())
            ]
        ),
        range: None
    }
}

fn supports_utf8_encoding(params: InitializeParams) -> bool {
    if let Some(general) = params.capabilities.general {
        if let Some(encodings) = general.position_encodings {
            for e in encodings {
                if e.as_str() == "utf-8" {
                    return true;
                }
            }
        }
    }
    return false;
}

// Note to self:
// To log a thing that can be deserialized with serde, we can do:
// serde_json::to_string_pretty(<thing>).unwrap()

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let mut encoding = PositionEncodingKind::UTF8;

        if !supports_utf8_encoding(params) {
            // In the case that the server does not support utf-8 position encodings:
            //
            // We just lie and say we support utf16, even though we don't. I really
            // do not care about actually supporting these clients correctly, even
            // though I have used VSCode a lot. I don't really respect the choice for
            // utf-16 here and I would rather simply have broken programs than implement
            // it myself.
            encoding = PositionEncodingKind::UTF16;
            self.client.log_message(
                MessageType::WARNING, 
                "Client doesn't support UTF-8 position encodings. Unicode projects will not work correctly.").await;
            // return Err(Error {
            //     code: ErrorCode::ServerError(0),
            //     message: Cow::from("PonieScript only supports utf-8 encoding"),
            //     data: None
            // });
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // We only support UTF-8 position encoding.
                position_encoding: Some(encoding),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions::default()),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::PARAMETER,
                                    SemanticTokenType::FUNCTION,
                                ],
                                token_modifiers: vec![],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            work_done_progress_options: Default::default(),
                        },
                    )
                ),

                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)) ,

                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        let mut print = CompletionItem::new_simple("print".to_string(), "Print out any series of expressions.".to_string());
        print.kind = Some(CompletionItemKind::FUNCTION);
        let mut str = CompletionItem::new_simple("str".to_string(), "Convert any series of expressions to a new StrBuf.".to_string());
        str.kind = Some(CompletionItemKind::FUNCTION);

        Ok(Some(CompletionResponse::Array(vec![
            print, str
        ])))
    }

    async fn hover(&self, _: HoverParams) -> Result<Option<Hover>> {
        Ok(Some(build_hover("print(args: ...)", "Prints any series of expressions.")))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        self.client.log_message(MessageType::INFO, format!("Semantic tokens requested for {}", params.text_document.uri)).await;

        let Ok(path) = params.text_document.uri.to_file_path() else {
            self.client.log_message(MessageType::INFO, format!("Unable to get Path as file: {}", params.text_document.uri)).await;
            return Ok(None);
        };

        let (mut db, ast, modules) = do_handle_files(path);

        let mut visitor = SemanticTokenVisitor { tokens: vec![], cursor_line: 0, cursor_start: 0 };

        for module in &modules {
            for fun in &module.functions {
                visitor.visit_expr(&ast, &mut db, fun.value);
            }
        }

        let tokens = SemanticTokens { result_id: None, data: visitor.tokens };

        self.client.log_message(MessageType::INFO, format!("Found {} semantic tokens", tokens.data.len())).await;
        Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        self.store.write().await.update(uri, text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().next() {
            let text = change.text;

            self.store.write().await.update(uri, text);
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        store: RwLock::new(DocumentStore::new())
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}