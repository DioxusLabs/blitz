use euclid::{Rect, Scale, Size2D};
use servo_url::ServoUrl;
use slab::Slab;
use style::{
    context::QuirksMode,
    dom::{TDocument, TElement, TNode, TShadowRoot},
    media_queries::MediaType,
    media_queries::{Device as StyleDevice, MediaList},
    servo_arc::Arc,
    shared_lock::SharedRwLock,
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
};

static QUIRKS_MODE: QuirksMode = QuirksMode::NoQuirks;

fn make_document_stylesheet() -> DocumentStyleSheet {
    DocumentStyleSheet(Arc::new(make_stylesheet()))
}

fn make_stylist() -> Stylist {
    Stylist::new(
        StyleDevice::new(
            MediaType::screen(),
            QUIRKS_MODE,
            Size2D::new(10.0, 10.0),
            Scale::new(1.0),
        ),
        QUIRKS_MODE,
    )
}

fn make_stylesheet() -> Stylesheet {
    let css = r#"
    body {
        background-color: red;
    }

    div {
        background-color: blue;
    }

    div:hover {
        background-color: green;
    }
    "#;

    let url_data = ServoUrl::from_url("data:text/css;charset=utf-8;base64,".parse().unwrap());
    let origin = Origin::UserAgent;
    let media_list = MediaList::empty();
    let shared_lock = SharedRwLock::new();
    let media = Arc::new(shared_lock.wrap(media_list));
    let stylesheet_loader = None;
    let allow_import_rules = AllowImportRules::Yes;

    style::stylesheets::Stylesheet::from_str(
        css,
        url_data,
        origin,
        media,
        shared_lock.clone(),
        stylesheet_loader,
        None,
        QUIRKS_MODE,
        0,
        allow_import_rules,
    )
}

#[tokio::test]
async fn render_simple() {
    let guard = SharedRwLock::new();
    let guards = style::shared_lock::StylesheetGuards {
        author: (),
        ua_or_user: (),
    };

    let stylesheet = make_document_stylesheet();

    let mut stylist = make_stylist();

    {
        let lock = guard.read();
        stylist.append_stylesheet(stylesheet, &lock);
    }

    stylist.compute_for_declarations(&guards, parent_style, declarations)
}

mod impls {
    #[derive(Clone, Copy)]
    struct Document {}

    impl TDocument for Document {
        type ConcreteNode = Concrete;

        fn as_node(&self) -> Self::ConcreteNode {
            todo!()
        }

        fn is_html_document(&self) -> bool {
            todo!()
        }

        fn quirks_mode(&self) -> QuirksMode {
            todo!()
        }

        fn shared_lock(&self) -> &SharedRwLock {
            todo!()
        }
    }

    struct Concrete {
        id: u64,
    }
}
