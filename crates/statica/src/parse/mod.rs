//! Owned HTML AST + html5ever bridge.
//!
//! Parse flow: [`pre`] (authoring normalize) → html5ever → [`normalize`] (AST lower).

mod ast;
mod html5;
mod normalize;
mod pre;
mod serialize;

pub use ast::{AttrMap, Document, Element, Node};
pub use html5::{parse_document, parse_fragment};
pub use serialize::{escape_text, serialize_document, serialize_nodes};
