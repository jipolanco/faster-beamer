//
// parsing.rs
// Copyright (C) 2019 stephan <stephan@stephan-ThinkPad-X300>
// Distributed under terms of the MIT license.
//

use crate::tree_traversal::get_nodes_of_type;
use std::fs;
use tree_sitter::{Node, Parser};
use tree_sitter_latex;

pub struct ParsedFile {
    pub filename: String,
    pub file_content: String,
    pub syntax_tree: tree_sitter::Tree,
}

impl ParsedFile {
    pub fn new(filename: String) -> ParsedFile {
        let file_content = fs::read_to_string(&filename).expect("Failed to read file");
        ParsedFile::from_string(filename, file_content)
    }

    pub fn from_string(filename: String, file_content: String) -> ParsedFile {
        let mut parser = Parser::new();
        let language = tree_sitter_latex::language();

        parser.set_language(language).unwrap();

        let tree = parser
            .parse(&file_content, None)
            .expect("Failed to parse file");
        ParsedFile {
            filename,
            file_content,
            syntax_tree: tree,
        }
    }

    pub fn get_nodes_of_type(&self, node_type: String) -> Vec<Node> {
        let root_node = self.syntax_tree.root_node();
        get_nodes_of_type(root_node, node_type, false)
    }

    pub fn get_node_string(&self, node: &Node) -> &str {
        &self.file_content[node.start_byte()..node.end_byte()]
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn print_nodes_of_language() {
        let languages = vec![tree_sitter_latex::language()];

        for l in languages {
            for i in 0..l.node_kind_count() {
                //println!("{}", l.node_kind_for_id(i as u16));
            }
        }
    }
}
