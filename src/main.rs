use std::env;

use tower_lsp::{LspService, Server};

mod backend;
mod code_action;
mod compat;
mod composer;
mod diagnostics;
mod file;
mod php_namespace;
mod scope;
mod types;

#[tokio::main]
async fn main() {
    if let Some(first_arg) = env::args().nth(1) {
        if &first_arg == "--version" {
            println!("PHP LSP version {}", env!("CARGO_PKG_VERSION"));
            return;
        }
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(backend::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
