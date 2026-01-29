mod document;
mod inlay_hint;
mod goto;
mod hover;
mod color;
mod completion;
mod semantic_locate;
mod semantic_tokens;
mod signature_help;
mod documentation;

use std::path::PathBuf;

use clap::Parser;
use poni_arena::IndexCell;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use poniescript_core::{
    db::*,
    expr::*,

    source::*
};

use crate::document::*;

struct Backend {
    client: Client,
    store: Mutex<DocumentStore>,
}

fn supports_utf8_encoding(params: &InitializeParams) -> bool {
    if let Some(general) = &params.capabilities.general {
        if let Some(encodings) = &general.position_encodings {
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

        if !supports_utf8_encoding(&params) {
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

        if let Some(uri) = &params.root_uri {
            if let Ok(path) = uri.to_file_path() {
                let maybe_poni_toml = path.join("ponies.toml");
                eprintln!("checking for possible ponies.toml: {}", maybe_poni_toml.display());
                if let Ok(as_uri) = Url::from_file_path(&maybe_poni_toml) {
                    if let Ok(text) = std::fs::read_to_string(&maybe_poni_toml) {
                        let mut doc = self.store.lock().await;
                        doc.initialize_projects_from_toml(&as_uri, text);
                    }
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // We only support UTF-8 position encoding.
                position_encoding: Some(encoding),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // Include @ as a trigger character for annotations
                    trigger_characters: Some(vec!["@".into(), ".".into()]),
                    ..Default::default()
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::PARAMETER,
                                    SemanticTokenType::FUNCTION,
                                    SemanticTokenType::PROPERTY,
                                    SemanticTokenType::METHOD,
                                ],
                                token_modifiers: vec![
                                    SemanticTokenModifier::READONLY,
                                ],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            work_done_progress_options: Default::default(),
                        },
                    )
                ),

                inlay_hint_provider: Some(OneOf::Left(true)),

                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),

                definition_provider: Some(OneOf::Left(true)),
                
                color_provider: Some(ColorProviderCapability::ColorProvider(ColorProviderOptions{})),

                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),

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

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let mut store = self.store.lock().await;
        Ok(completion::completion(&mut store, params))
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
        // Disable semantic tokens for now. They're not very useful and the LSP
        // is pretty unstable.
        //const ENABLE_SEMANTIC_TOKEN_SUPPORT: bool = false;
        //if !ENABLE_SEMANTIC_TOKEN_SUPPORT { return Ok(None); }

        //  TODO: Make semantic tokens work with new Project setup.
        // self.client.log_message(MessageType::INFO, format!("Semantic tokens requested for {}", params.text_document.uri)).await;
         let mut store = self.store.lock().await;
        Ok(semantic_tokens::semantic_tokens(&mut store, params))
        
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

    async fn signature_help(
        &self,
        params: SignatureHelpParams
    ) -> Result<Option<SignatureHelp>> {
        let mut store = self.store.lock().await;
        Ok(signature_help::signature_help(&mut store, params))
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        let lock = self.store.lock().await;

        let Some(project) = lock.projects.get(&params.text_document.uri) else {
            return Ok(Vec::new());
        };

        let proj = project.get_cache(&lock);
        let proj = proj.lock().unwrap();

        let Some(document) = proj.url_to_id_map.get(&params.text_document.uri) else {
            return Ok(Vec::new());
        };

        let mut colors = Vec::new();

        for color in &proj.db.color_tokens {
            let mut location = color.location.clone();
            // Collect only color tokens for this document.
            // Note that we expect there to be pretty few color tokens overall,
            // so it shouldn't be hugely inefficient to filter them this way.
            if location.source != *document { continue; }
            // Cut out the (#)
            location.offset += 2;
            location.length -= 3;
            colors.push(ColorInformation {
                range: convert_range(&proj.ast, &location),
                color: color::convert_color(&proj.db, color)
            })
        }
       
        Ok(colors)
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        // For now, as a hack, determine whether we want a 3-elem color or a 
        // 4-elem color based on the approximate length of the range.
        let approx_len = params.range.end.character - params.range.start.character;

        let elems = match approx_len {
            3 => 3,
            4 => 4,
            6 => 3,
            8 => 4,
            _ => 4
        };

        let r = (params.color.red   * 255.0) as u8;
        let g = (params.color.green * 255.0) as u8;
        let b = (params.color.blue  * 255.0) as u8;
        let a = (params.color.alpha * 255.0) as u8;

        let label = match elems {
            3 => format!("{:02x}{:02x}{:02x}", r, g, b),
            4 => format!("{:02x}{:02x}{:02x}{:02x}", r, g, b, a),
            _ => unreachable!()
        };

        Ok(vec![ColorPresentation {
            label,
            text_edit: None,
            additional_text_edits: None
        }])
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