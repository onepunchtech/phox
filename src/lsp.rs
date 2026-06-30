use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::ast::Span;
use crate::elaborate;
use crate::parser;
use crate::Phox;

/// Extra diagnostics callback: given the evaluated Val, return domain-specific errors.
pub type DiagnosticsFn = Box<dyn Fn(&crate::Val) -> Vec<String> + Send + Sync>;

struct PhoxBackend {
    client: Client,
    documents: Mutex<HashMap<Url, String>>,
    /// Extra modules to register on each Phox instance (for DSL tool support).
    extra_modules: Vec<(String, String)>,
    /// Optional domain-specific validation after phox eval.
    extra_diagnostics: Option<DiagnosticsFn>,
}

impl PhoxBackend {
    fn get_document(&self, uri: &Url) -> Option<String> {
        self.documents.lock().unwrap().get(uri).cloned()
    }

    fn set_document(&self, uri: Url, text: String) {
        self.documents.lock().unwrap().insert(uri, text);
    }

    fn make_phox(&self) -> Phox {
        let mut phox = Phox::new();
        for (path, source) in &self.extra_modules {
            phox = phox.with_module(path.clone(), source.clone());
        }
        phox
    }

    fn file_dir(&self, uri: &Url) -> PathBuf {
        uri.to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    async fn publish_diagnostics(&self, uri: Url) {
        let source = match self.get_document(&uri) {
            Some(s) => s,
            None => return,
        };

        let base_dir = self.file_dir(&uri);
        let phox = self.make_phox();
        let diagnostics = compute_diagnostics(&phox, &source, &base_dir, &self.extra_diagnostics);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

fn compute_diagnostics(
    phox: &Phox,
    source: &str,
    base_dir: &PathBuf,
    extra_diagnostics: &Option<DiagnosticsFn>,
) -> Vec<Diagnostic> {
    // Try full eval with imports — catches both parse and elab errors
    match phox.eval_with_imports(source, base_dir) {
        Ok(result) => {
            // Run domain-specific validation if provided
            if let Some(validate) = extra_diagnostics {
                let errors = validate(&result.val);
                return errors
                    .into_iter()
                    .map(|msg| {
                        // Try to find the field name in source for better positioning
                        let range = find_field_in_source(source, &msg)
                            .unwrap_or_else(|| {
                                let last_line = source.lines().count().saturating_sub(1) as u32;
                                Range::new(Position::new(0, 0), Position::new(last_line, 0))
                            });
                        Diagnostic {
                            range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("phox".into()),
                            message: msg,
                            ..Default::default()
                        }
                    })
                    .collect();
            }
            Vec::new()
        }
        Err(e) => {
            match &e {
                crate::PhoxError::Parse(errors) => {
                    errors
                        .iter()
                        .map(|e| Diagnostic {
                            range: span_to_range(source, e.span),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: e.message.clone(),
                            ..Default::default()
                        })
                        .collect()
                }
                crate::PhoxError::Elab(e) => {
                    let range = match e.span {
                        Some(span) => span_to_range(source, span),
                        None => Range::new(Position::new(0, 0), Position::new(0, 0)),
                    };
                    vec![Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: format!("{}", e.error),
                        ..Default::default()
                    }]
                }
                _ => {
                    vec![Diagnostic {
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: format!("{e}"),
                        ..Default::default()
                    }]
                }
            }
        }
    }
}

/// Try to locate a field name from an error message like "unknown field 'command'"
/// or "missing field 'version'" in the source text.
fn find_field_in_source(source: &str, msg: &str) -> Option<Range> {
    // Extract field name from patterns like "'fieldname'"
    let field = msg.split('\'').nth(1)?;

    // Search for `field =` or `field :` pattern in source
    for (line_num, line) in source.lines().enumerate() {
        // Look for the field name followed by = or : (record field assignment)
        if let Some(col) = line.find(field) {
            let after = &line[col + field.len()..].trim_start();
            if after.starts_with('=') || after.starts_with(':') {
                return Some(Range::new(
                    Position::new(line_num as u32, col as u32),
                    Position::new(line_num as u32, (col + field.len()) as u32),
                ));
            }
        }
    }
    None
}

fn compute_hover(source: &str, position: Position) -> Option<String> {
    let offset = position_to_offset(source, position);

    let expr = parser::parse(source).ok()?;
    let mcxt = elaborate::MetaCxt::new();
    let cxt = elaborate::Cxt::new_with_hover(&mcxt);

    let _ = elaborate::infer(&cxt, &expr);

    let map = cxt.hover_map.as_ref()?.borrow();

    let mut best: Option<&(Span, String)> = None;
    for entry in map.iter() {
        let (span, _) = entry;
        if span.0 <= offset && offset <= span.1 {
            match best {
                None => best = Some(entry),
                Some((best_span, _)) => {
                    let best_width = best_span.1 - best_span.0;
                    let this_width = span.1 - span.0;
                    if this_width < best_width {
                        best = Some(entry);
                    }
                }
            }
        }
    }

    best.map(|(_, ty)| ty.clone())
}

fn offset_to_position(source: &str, offset: usize) -> Position {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() as u32;
    let col = before.len() - before.rfind('\n').map_or(0, |i| i + 1);
    Position::new(line, col as u32)
}

fn position_to_offset(source: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in source.lines().enumerate() {
        if i == pos.line as usize {
            return offset + (pos.character as usize).min(line.len());
        }
        offset += line.len() + 1;
    }
    source.len()
}

fn span_to_range(source: &str, span: Span) -> Range {
    Range::new(
        offset_to_position(source, span.0),
        offset_to_position(source, span.1),
    )
}

#[tower_lsp::async_trait]
impl LanguageServer for PhoxBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Phox LSP initialized")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        self.set_document(uri.clone(), params.text_document.text);
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().last() {
            self.set_document(uri.clone(), change.text);
        }
        self.publish_diagnostics(uri).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_diagnostics(params.text_document.uri).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let source = match self.get_document(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let type_str = match compute_hover(&source, pos) {
            Some(s) => s,
            None => return Ok(None),
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```phox\n{type_str}\n```"),
            }),
            range: None,
        }))
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let source = match self.get_document(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        match crate::format::format(&source) {
            Ok(formatted) => {
                let last_line = source.matches('\n').count() as u32;
                let last_col =
                    source.len() - source.rfind('\n').map_or(0, |i| i + 1);
                Ok(Some(vec![TextEdit {
                    range: Range::new(
                        Position::new(0, 0),
                        Position::new(last_line, last_col as u32),
                    ),
                    new_text: formatted,
                }]))
            }
            Err(_) => Ok(None),
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

pub async fn run_server() {
    run_server_with(Vec::new(), None).await;
}

/// Run the LSP server with extra embedded modules and optional domain validation.
pub async fn run_server_with(
    extra_modules: Vec<(String, String)>,
    extra_diagnostics: Option<DiagnosticsFn>,
) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| PhoxBackend {
        client,
        documents: Mutex::new(HashMap::new()),
        extra_modules,
        extra_diagnostics,
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
