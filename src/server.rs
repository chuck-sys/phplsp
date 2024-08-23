use tower_lsp::Client;
use tower_lsp::lsp_types::*;

use async_channel::{Receiver, Sender};

use tree_sitter::{Parser, Tree, Node};

use std::collections::HashMap;

use crate::msg::{MsgFromServer, MsgToServer};

pub struct Server {
    client: Client,
    sender_to_backend: Sender<MsgFromServer>,
    receiver_from_backend: Receiver<MsgToServer>,
    parser: Parser,

    file_trees: HashMap<Url, Tree>,
}

fn range_plaintext(file_contents: &String, range: tree_sitter::Range) -> String {
    file_contents[range.start_byte..range.end_byte].to_owned()
}

fn document_symbols(uri: &Url, root_node: &Node, file_contents: &String) -> Vec<SymbolInformation> {
    let mut ret = Vec::new();
    let mut cursor = root_node.walk();

    while cursor.goto_first_child() {
        loop {
            let kind = cursor.node().kind();
            if kind == "class_declaration" {
                if let Some(name_node) = cursor.node().child_by_field_name("name") {
                    ret.push(SymbolInformation {
                        name: range_plaintext(file_contents, name_node.range()),
                        kind: SymbolKind::CLASS,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: cursor.node().range().start_point.row as u32,
                                    character: cursor.node().range().start_point.column as u32,
                                },
                                end: Position {
                                    line: cursor.node().range().end_point.row as u32,
                                    character: cursor.node().range().end_point.column as u32,
                                },
                            },
                        },
                        container_name: None,
                    });
                }
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    ret
}

impl Server {
    pub fn new(client: Client, sx: Sender<MsgFromServer>, rx: Receiver<MsgToServer>) -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_php::language_php()).expect("error loading PHP grammar");

        Self {
            client,
            sender_to_backend: sx,
            receiver_from_backend: rx,
            parser,

            file_trees: HashMap::new(),
        }
    }

    pub async fn serve(&mut self) {
        self.client.log_message(MessageType::LOG, "starting to serve").await;

        loop {
            match self.receiver_from_backend.recv_blocking() {
                Ok(msg) => match msg {
                    MsgToServer::Shutdown => break,
                    MsgToServer::DidOpen { url, text, version } => self.did_open(url, text, version).await,
                    MsgToServer::DocumentSymbol(url) => self.document_symbol(url).await,
                    _ => unimplemented!(),
                },
                Err(e) => self.client.log_message(MessageType::ERROR, e).await,
            }
        }
    }

    async fn did_open(&mut self, url: Url, text: String, version: i32) {
        match self.parser.parse(text, None) {
            Some(tree) => {
                self.file_trees.insert(url, tree);
            },
            None => self.client.log_message(MessageType::ERROR, format!("could not parse file `{}`", &url)).await,
        }
    }

    async fn document_symbol(&mut self, url: Url) {
        if let Some(tree) = self.file_trees.get(&url) {
            // if let Err(e) = self.sender_to_backend.send(MsgFromServer::FlatSymbols(symbols(&url, &tree.root_node()))).await {
            //     self.client.log_message(MessageType::ERROR, format!("document_symbol: unable to send to backend: {}", e)).await;
            // }
        } else {
            if let Err(e) = self.sender_to_backend.send(MsgFromServer::FlatSymbols(vec![])).await {
                self.client.log_message(MessageType::ERROR, format!("document_symbol: unable to send; no file `{}`: {}", &url, e)).await;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use tree_sitter::Parser;
    use tower_lsp::lsp_types::*;

    use super::document_symbols;

    #[test]
    fn test_get_symbols() {
        let source = "<?php\nclass Whatever {\npublic int $x;\n}";
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_php::language_php()).expect("error loading PHP grammar");

        let tree = parser.parse(source, None).unwrap();
        let root_node = tree.root_node();
        let uri = Url::from_file_path("/home/file.php").unwrap();
        let actual_symbols = document_symbols(&uri, &root_node, &source.to_string());
        assert_eq!(actual_symbols[0], SymbolInformation {
            name: "Whatever".to_string(),
            kind: SymbolKind::CLASS,
            tags: None,
            deprecated: None,
            location: Location {
                uri: Url::from_file_path("/home/file.php").unwrap(),
                range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 3,
                        character: 1,
                    },
                },
            },
            container_name: None,
        });
    }
}
