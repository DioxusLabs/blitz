#[cfg(target_os = "ios")]
fn main() {
    use anyrender_vello_hybrid::VelloHybridWindowRenderer;
    use blitz_dom::{DocumentConfig, UserAgentStylesheet};
    use blitz_html::HtmlDocument;
    use blitz_shell::{BlitzApplication, BlitzShellProxy, WindowConfig, create_default_event_loop};

    let doc = HtmlDocument::from_html(
        include_str!("index.html"),
        DocumentConfig {
            ua_stylesheets: vec![UserAgentStylesheet::default_sheet()],
            ..Default::default()
        },
    );

    let event_loop = create_default_event_loop();
    let (proxy, receiver) = BlitzShellProxy::new(event_loop.create_proxy());
    let mut app = BlitzApplication::new(proxy, receiver);
    app.add_window(WindowConfig::new(
        Box::new(doc),
        VelloHybridWindowRenderer::new(),
    ));
    event_loop.run_app(app).unwrap();
}

#[cfg(not(target_os = "ios"))]
fn main() -> std::process::ExitCode {
    eprintln!("Run this example in the iOS Simulator. See examples/ios_scene/README.md.");
    std::process::ExitCode::FAILURE
}
