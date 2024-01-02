// #[derive(Debug)]
// pub enum NodeData {
//     /// The `Document` itself - the root node of a HTML document.
//     Document,

//     /// A `DOCTYPE` with name, public id, and system id. See
//     /// [document type declaration on wikipedia][dtd wiki].
//     ///
//     /// [dtd wiki]: https://en.wikipedia.org/wiki/Document_type_declaration
//     Doctype {
//         name: StrTendril,
//         public_id: StrTendril,
//         system_id: StrTendril,
//     },

//     /// A text node.
//     Text { contents: RefCell<StrTendril> },

//     /// A comment.
//     Comment { contents: StrTendril },

//     /// An element with attributes.
//     Element {
//         name: QualName,
//         attrs: RefCell<Vec<Attribute>>,

//         /// For HTML \<template\> elements, the [template contents].
//         ///
//         /// [template contents]: https://html.spec.whatwg.org/multipage/#template-contents
//         template_contents: RefCell<Option<()>>,
//         // template_contents: RefCell<Option<Handle>>,
//         /// Whether the node is a [HTML integration point].
//         ///
//         /// [HTML integration point]: https://html.spec.whatwg.org/multipage/#html-integration-point
//         mathml_annotation_xml_integration_point: bool,
//     },

//     /// A Processing instruction.
//     ProcessingInstruction {
//         target: StrTendril,
//         contents: StrTendril,
//     },
// }
