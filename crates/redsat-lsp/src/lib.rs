//! The RedSAT language server.
//!
//! `redsat lsp` speaks the Language Server Protocol over stdio. Documents are analysed on every
//! change with [`redsat_lang::analyze`]: parsed, type-checked and elaborated up to domain
//! inference, but never solved. That powers live diagnostics, completion, hovers showing each
//! unknown's inferred domain and bit width, go-to-definition, the outline and inlay hints.

mod document;
mod features;

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, InlayHintRequest, Request as _,
};
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InlayHintParams, Location, MarkupContent, MarkupKind,
    OneOf, PublishDiagnosticsParams, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
};

pub use document::{Document, LineIndex};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Runs the server on stdin/stdout until the client shuts it down.
///
/// # Errors
///
/// Protocol and I/O failures.
pub fn run_stdio() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), ":".into()]),
            ..CompletionOptions::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    };
    let initialize = serde_json::json!({
        "capabilities": capabilities,
        "serverInfo": ServerInfo { name: "redsat".into(), version: Some(env!("CARGO_PKG_VERSION").into()) },
    });
    let (id, _params) = connection.initialize_start()?;
    connection.initialize_finish(id, initialize)?;
    Server {
        connection: &connection,
        documents: HashMap::new(),
    }
    .run()?;
    // The writer thread finishes only once every sender is gone.
    drop(connection);
    io_threads.join()?;
    Ok(())
}

struct Server<'a> {
    connection: &'a Connection,
    documents: HashMap<String, Document>,
}

impl Server<'_> {
    fn run(&mut self) -> Result<()> {
        for message in &self.connection.receiver {
            match message {
                Message::Request(request) => {
                    if self.connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    let response = self.request(request);
                    self.connection.sender.send(Message::Response(response))?;
                }
                Message::Notification(notification) => self.notification(notification)?,
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn request(&self, request: Request) -> Response {
        let id = request.id.clone();
        let result = match request.method.as_str() {
            Completion::METHOD => self.handle::<CompletionParams, _>(
                request,
                |doc, params| {
                    let offset = doc.offset(params.text_document_position.position);
                    Some(CompletionResponse::Array(features::completions(
                        doc, offset,
                    )))
                },
                |params| &params.text_document_position.text_document.uri,
            ),
            HoverRequest::METHOD => self.handle::<HoverParams, _>(
                request,
                |doc, params| {
                    let offset = doc.offset(params.text_document_position_params.position);
                    features::hover(doc, offset).map(|(range, value)| Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value,
                        }),
                        range: Some(range),
                    })
                },
                |params| &params.text_document_position_params.text_document.uri,
            ),
            GotoDefinition::METHOD => {
                let uri = serde_json::from_value::<GotoDefinitionParams>(request.params.clone())
                    .map(|p| p.text_document_position_params.text_document.uri);
                self.handle::<GotoDefinitionParams, _>(
                    request,
                    |doc, params| {
                        let offset = doc.offset(params.text_document_position_params.position);
                        let uri = uri.as_ref().ok()?.clone();
                        features::definition(doc, offset).map(|span| {
                            GotoDefinitionResponse::Scalar(Location::new(uri, doc.range(span)))
                        })
                    },
                    |params| &params.text_document_position_params.text_document.uri,
                )
            }
            DocumentSymbolRequest::METHOD => self.handle::<DocumentSymbolParams, _>(
                request,
                |doc, params| {
                    Some(DocumentSymbolResponse::Flat(features::symbols(
                        doc,
                        &params.text_document.uri,
                    )))
                },
                |params| &params.text_document.uri,
            ),
            InlayHintRequest::METHOD => self.handle::<InlayHintParams, _>(
                request,
                |doc, params| Some(features::inlay_hints(doc, params.range)),
                |params| &params.text_document.uri,
            ),
            _ => Ok(serde_json::Value::Null),
        };
        match result {
            Ok(value) => Response::new_ok(id, value),
            Err(message) => {
                Response::new_err(id, lsp_server::ErrorCode::InvalidParams as i32, message)
            }
        }
    }

    /// Decodes a request's parameters, finds its document and runs `feature` on it.
    fn handle<P, R>(
        &self,
        request: Request,
        feature: impl FnOnce(&Document, &P) -> Option<R>,
        uri_of: impl FnOnce(&P) -> &Uri,
    ) -> std::result::Result<serde_json::Value, String>
    where
        P: serde::de::DeserializeOwned,
        R: serde::Serialize,
    {
        let params: P =
            serde_json::from_value(request.params).map_err(|error| error.to_string())?;
        let Some(document) = self.documents.get(uri_of(&params).as_str()) else {
            return Ok(serde_json::Value::Null);
        };
        Ok(
            feature(document, &params).map_or(serde_json::Value::Null, |result| {
                serde_json::to_value(result).unwrap_or_default()
            }),
        )
    }

    fn notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: lsp_types::DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let item = params.text_document;
                self.update(item.uri, item.version, item.text)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                if let Some(change) = params.content_changes.into_iter().last() {
                    self.update(
                        params.text_document.uri,
                        params.text_document.version,
                        change.text,
                    )?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: lsp_types::DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.documents.remove(params.text_document.uri.as_str());
                self.publish(params.text_document.uri, Vec::new(), None)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn update(&mut self, uri: Uri, version: i32, text: String) -> Result<()> {
        let previous = self.documents.remove(uri.as_str());
        let document = Document::update(previous, version, text, file_path(&uri));
        let diagnostics = features::diagnostics(&document, &uri);
        self.documents.insert(uri.as_str().to_string(), document);
        self.publish(uri, diagnostics, Some(version))
    }

    fn publish(
        &self,
        uri: Uri,
        diagnostics: Vec<lsp_types::Diagnostic>,
        version: Option<i32>,
    ) -> Result<()> {
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version,
        };
        let notification = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
        self.connection
            .sender
            .send(Message::Notification(notification))?;
        Ok(())
    }
}

/// The local path of a `file://` URI (percent-decoded), used to find `redsat.toml`. URIs that
/// reach a server inside WSL from Windows are mapped too: `file:///c%3A/…` to `/mnt/c/…` and
/// `file://wsl.localhost/<distribution>/…` (or `wsl$`) to `/…`.
fn file_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let path = percent_decode(path)?;
    if host.eq_ignore_ascii_case("wsl.localhost") || host.eq_ignore_ascii_case("wsl$") {
        // Drop the distribution name: `/Ubuntu/home/me` → `/home/me`.
        let inner = &path[1..];
        return Some(PathBuf::from(&inner[inner.find('/')?..]));
    }
    if !host.is_empty() {
        return None;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        let drive = (bytes[1] as char).to_ascii_lowercase();
        return Some(PathBuf::from(format!("/mnt/{drive}{}", &path[3..])));
    }
    Some(PathBuf::from(path))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_uris() {
        let uri: Uri = "file:///home/me/My%20Puzzles/a.rsat".parse().unwrap();
        assert_eq!(
            file_path(&uri),
            Some(PathBuf::from("/home/me/My Puzzles/a.rsat"))
        );
        let untitled: Uri = "untitled:Untitled-1".parse().unwrap();
        assert_eq!(file_path(&untitled), None);
        let windows: Uri = "file:///c%3A/Users/me/a.rsat".parse().unwrap();
        assert_eq!(
            file_path(&windows),
            Some(PathBuf::from("/mnt/c/Users/me/a.rsat"))
        );
        let bridged: Uri = "file://wsl.localhost/Ubuntu/home/me/a.rsat"
            .parse()
            .unwrap();
        assert_eq!(file_path(&bridged), Some(PathBuf::from("/home/me/a.rsat")));
    }
}
