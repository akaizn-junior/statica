//! Owned HTML AST + html5ever bridge.

mod ast;
mod html5;
mod serialize;

pub use ast::{AttrMap, Document, Element, Node};
pub use html5::{parse_document, parse_fragment};
pub use serialize::{escape_text, serialize_document, serialize_nodes};
