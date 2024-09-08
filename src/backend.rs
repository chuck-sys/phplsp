use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use async_channel::{Receiver, Sender};

use crate::msg::{MsgFromServer, MsgToServer};

pub struct Backend {
    client: Client,
    receiver_from_server: Receiver<MsgFromServer>,
    sender_to_server: Sender<MsgToServer>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        let (sender_to_backend, receiver_from_server) = async_channel::unbounded();
        let (sender_to_server, receiver_from_backend) = async_channel::unbounded();
        let mut server = crate::server::Server::new(client.clone(), sender_to_backend, receiver_from_backend);
        std::thread::spawn(move || {
            async move {
                server.serve().await;
            }
        });

        Self {
            client,
            receiver_from_server,
            sender_to_server,
        }
    }

    pub async fn send(&self, msg: MsgToServer) {
        if let Err(x) = self.sender_to_server.send(msg).await {
            self.client.log_message(MessageType::ERROR, x).await;
        }
    }

    pub async fn recv(&self) -> Option<MsgFromServer> {
        match self.receiver_from_server.recv().await {
            Ok(msg) => Some(msg),
            Err(x) => {
                self.client.log_message(MessageType::ERROR, x).await;
                None
            },
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let mut workspace_folders = params.workspace_folders.unwrap_or(vec![]);
        if workspace_folders.len() == 0 {
            if let Some(root_uri) = params.root_uri {
                workspace_folders.push(WorkspaceFolder {
                    uri: root_uri.clone(),
                    name: root_uri.to_string()
                });
            } else {
                // deprecated options, but we should check for them nonetheless
                if let Some(root_path) = params.root_path {
                    if let Ok(root_uri) = Url::from_directory_path(&root_path) {
                        workspace_folders.push(WorkspaceFolder {
                            uri: root_uri,
                            name: root_path
                        });
                    }
                }
            }
        }

        if workspace_folders.len() == 0 {
            self.client
                .log_message(MessageType::LOG, "unable to find workspace folders, root paths, or root uris")
                .await;
        } else {
            self.client
                .log_message(MessageType::LOG, format!("found {} workspace folders: {:?}", workspace_folders.len(), &workspace_folders))
                .await;
        }

        // TODO check workspace folders for `composer.json` and read namespaces with PSR-4 and
        // PSR-0 (maybe support it??)

        Ok(InitializeResult {
            capabilities: ServerCapabilities::default(),
            server_info: Some(ServerInfo {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        self.send(MsgToServer::Shutdown).await;
        self.client.log_message(MessageType::LOG, "server thread has shutdown").await;
        Ok(())
    }

    async fn did_open(&self, data: DidOpenTextDocumentParams) {
        self.send(MsgToServer::DidOpen {
            url: data.text_document.uri,
            text: data.text_document.text,
            version: data.text_document.version,
        }).await;
    }

    async fn document_symbol(&self, data: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>> {
        self.send(MsgToServer::DocumentSymbol(data.text_document.uri)).await;

        match self.recv().await {
            Some(MsgFromServer::NestedSymbols(symbols)) => Ok(Some(DocumentSymbolResponse::Nested(symbols))),
            Some(MsgFromServer::FlatSymbols(symbols)) => Ok(Some(DocumentSymbolResponse::Flat(symbols))),
            _ => Ok(None),
        }
    }
}
