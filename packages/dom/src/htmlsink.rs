use std::{borrow::Cow, pin::Pin};

use crate::node::Node;
use html5ever::{
    tendril::{StrTendril, TendrilSink},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
    Attribute, ExpandedName, QualName,
};
use slab::Slab;
use style::{
    dom::{TDocument, TNode},
    shared_lock::SharedRwLock,
};

// struct HtmlConsumer<'a> {
//     tree: Pin<&'a mut Tree>,

//     /// Errors that occurred during parsing.
//     pub errors: Vec<Cow<'static, str>>,

//     /// The document's quirks mode.
//     pub quirks_mode: QuirksMode,
// }

// impl<'b> TreeSink for HtmlConsumer<'b> {
//     type Output = Self;

//     // we use the ID of the nodes in the tree as the handle
//     type Handle = usize;

//     fn finish(self) -> Self::Output {
//         self
//     }

//     fn parse_error(&mut self, msg: Cow<'static, str>) {
//         self.errors.push(msg);
//     }

//     fn get_document(&mut self) -> Self::Handle {
//         0
//     }

//     fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> ExpandedName<'a> {
//         unimplemented!()
//     }

//     fn create_element(
//         &mut self,
//         name: QualName,
//         attrs: Vec<Attribute>,
//         flags: ElementFlags,
//     ) -> Self::Handle {
//         unimplemented!()
//     }

//     fn create_comment(&mut self, text: StrTendril) -> Self::Handle {
//         unimplemented!()
//     }

//     fn create_pi(&mut self, target: StrTendril, data: StrTendril) -> Self::Handle {
//         unimplemented!()
//     }

//     fn append(&mut self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
//         unimplemented!()
//     }

//     fn append_based_on_parent_node(
//         &mut self,
//         element: &Self::Handle,
//         prev_element: &Self::Handle,
//         child: NodeOrText<Self::Handle>,
//     ) {
//         unimplemented!()
//     }

//     fn append_doctype_to_document(
//         &mut self,
//         name: StrTendril,
//         public_id: StrTendril,
//         system_id: StrTendril,
//     ) {
//         unimplemented!()
//     }

//     fn get_template_contents(&mut self, target: &Self::Handle) -> Self::Handle {
//         unimplemented!()
//     }

//     fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
//         unimplemented!()
//     }

//     fn set_quirks_mode(&mut self, mode: QuirksMode) {
//         self.quirks_mode = mode;
//     }

//     fn append_before_sibling(
//         &mut self,
//         sibling: &Self::Handle,
//         new_node: NodeOrText<Self::Handle>,
//     ) {
//         unimplemented!()
//     }

//     fn add_attrs_if_missing(&mut self, target: &Self::Handle, attrs: Vec<Attribute>) {
//         unimplemented!()
//     }

//     fn remove_from_parent(&mut self, target: &Self::Handle) {
//         unimplemented!()
//     }

//     fn reparent_children(&mut self, node: &Self::Handle, new_parent: &Self::Handle) {
//         unimplemented!()
//     }
// }

// #[test]
// fn parses_some_html() {
//     let html = "<html><body><h1>hello world</h1></body></html>";
//     let mut tree = Tree::new();

//     let consumer = HtmlConsumer {
//         tree: tree.as_mut(),
//         errors: vec![],
//         quirks_mode: QuirksMode::NoQuirks,
//     };

//     html5ever::parse_document(consumer, Default::default())
//         .from_utf8()
//         .read_from(&mut html.as_bytes())
//         .unwrap();

//     // Now our tree should have some nodes in it
// }
