use blitz_dom::{BaseDocument, DocumentConfig};

/// Smoke-test: resolve on an empty document must not panic.
#[test]
fn resolve_empty_document_does_not_panic() {
    let mut doc = BaseDocument::new(DocumentConfig::default());
    doc.resolve(0.0);
    doc.resolve(0.1);
}
