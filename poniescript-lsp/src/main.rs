mod document;
mod inlay_hint;
mod goto;
mod hover;

use std::path::PathBuf;

use clap::Parser;
use poniescript_core::arena::IndexCell;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use poniescript_core::{
    db::*,
    expr::*,

    source::*
};

use crate::document::*;



struct SemanticTokenVisitor {
    tokens: Vec<SemanticToken>,
    cursor_start: u64,
    cursor_line: u64,
}

impl SemanticTokenVisitor {
    fn push_token(&mut self, ast: &Ast, _db: &Db, location: &SourceLocation, token_type: u32, token_modifiers_bitset: u32) {
        let (line, col) = ast.sources.get(location.source).get_line_column(location);
        let (line, col) = (line - 1, col - 1);

        let mut delta_line: u32 = 0;
        let delta_start: u32;

        if line == self.cursor_line {
            delta_start = (col - self.cursor_start) as u32;
        }
        else {
            if line < self.cursor_line {
                // TODO: Apparently our visit is not necessarily in-order.
                // We need to do stuff to fix that (probably sort all the tokens)
                eprintln!("bad semantic token");
                return;
            }
            delta_line = (line - self.cursor_line) as u32;
            delta_start = col as u32;
        }

        self.cursor_line = line;
        self.cursor_start = col;

        eprintln!("{}:{}: length: {}", self.cursor_line, self.cursor_start, location.length);

        self.tokens.push(SemanticToken { delta_line, delta_start, length: location.length as u32, token_type, token_modifiers_bitset });
    }

    fn push_var(&mut self, ast: &Ast, db: &Db, location: &SourceLocation, id: VarId) {
        let is_param = db.get(id).fun.is_some();

        self.push_token(ast, db, location, if is_param { 1 } else { 0 }, 0);
    }

    fn push_fun(&mut self, ast: &Ast, db: &Db, location: &SourceLocation) {
        eprintln!("push fun: {}", location.length);
        self.push_token(ast, db, location, 2, 0);
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

        self.push_var(ast, db, &assign.var_name, assign.identity);
        self.visit_expr(ast, db, assign.value);
    }

    fn visit_variable(&mut self,ast: &Ast, db: &mut Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let var = into!(binding.as_ref(), Variable);

        self.push_var(ast, db, &var.location, var.identity);
    }

    fn visit_funcapture(&mut self,ast: &Ast, db: &mut Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let capt = into!(binding.as_ref(), FunCapture);

        self.push_fun(ast, db, &capt.fn_name);
    }

    fn visit_funcall(&mut self,ast: &Ast, db: &mut Db,id:ExprId) {
        let binding = ast.get_expr(id);
        let call = into!(binding.as_ref(), FunCall);

        self.push_fun(ast, db, &call.fn_name);

        for arg in &call.args {
            self.visit_expr(ast, db, *arg);
        }
    }
}

struct Backend {
    client: Client,
    store: Mutex<DocumentStore>,
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



fn in_range(position: &Position, range: &Range) -> bool {
    if position.line < range.start.line { return false; }
    if position.line > range.end.line { return false; }

    if position.line == range.start.line && position.character < range.start.character { return false; }
    if position.line == range.end.line && position.character > range.end.character { return false; }

    return true;
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
                semantic_tokens_provider: None, /*Some(
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
                ),*/

                inlay_hint_provider: Some(OneOf::Left(true)),

                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),

                definition_provider: Some(OneOf::Left(true)),

                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized!")
            .await;
        
        let lock = self.store.lock().await;
        if let Some(c_output) = &lock.c_output {
            self.client
                .log_message(MessageType::INFO, format!("writing to c file: {}", c_output.display()))
                .await;
        }
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

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        //Ok(Some(build_hover("print(args: ...)", "Prints any series of expressions.")))
        let mut store = self.store.lock().await;
        Ok(hover::hover(&mut store, params))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let range = params.range;
        let mut lock = self.store.lock().await;
        let Some(cache) = lock.get_inlay_hint_cache(&params.text_document.uri) else {
            return Ok(None);
        };

        let mut overlay = vec![];
        for hint in &cache.hints {
            if in_range(&hint.position, &range) {
                overlay.push(hint.clone())
            }
        }

        Ok(Some(overlay))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        return Ok(None);
        // Disable semantic tokens for now. They're not very useful and the LSP
        // is pretty unstable.
        //const ENABLE_SEMANTIC_TOKEN_SUPPORT: bool = false;
        //if !ENABLE_SEMANTIC_TOKEN_SUPPORT { return Ok(None); }

        //  TODO: Make semantic tokens work with new Project setup.
        // self.client.log_message(MessageType::INFO, format!("Semantic tokens requested for {}", params.text_document.uri)).await;

        // let mut lock = self.store.lock().await;
        
        // let (db, ast, ..) = lock.get_cached_stuff();

        // let mut visitor = SemanticTokenVisitor { tokens: vec![], cursor_line: 0, cursor_start: 0 };

        // for source in ast.sources.iter() {
        //     let source = ast.sources.get(source);
        //     let module = &source.module;
        //     for fun in &module.functions {
        //         visitor.visit_expr(&ast, db, fun.value);
        //     }
        // }

        // let tokens = SemanticTokens { result_id: None, data: visitor.tokens };

        // self.client.log_message(MessageType::INFO, format!("Found {} semantic tokens", tokens.data.len())).await;
        // Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        self.store.lock().await.update(&uri, text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().next() {
            let text = change.text;

            let mut lock = self.store.lock().await;
            lock.update(&uri, text);

            let diag = lock.steal_diagnostics(&uri);
            drop(lock);

            if let Some(diag) = diag {
                for diag in diag.all {
                    self.client.publish_diagnostics(diag.0, diag.1, None).await;
                }
            }
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let mut lock = self.store.lock().await;
        Ok(goto::goto_definition(&mut lock, params))
    }
}

#[derive(Parser)]
struct LspArgs {
    /// An optional file path to write generated C code to whenever the input
    /// document changes. Primarily used for hot-code reloading.
    #[arg(short = 'o', long)]
    c_output: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let args = LspArgs::parse();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        store: Mutex::new(DocumentStore::new(&args))
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}