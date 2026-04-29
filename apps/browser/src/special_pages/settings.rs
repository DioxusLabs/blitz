use super::{SpecialPage, SpecialPageCtx, page_shell};

pub struct Settings;

impl SpecialPage for Settings {
    fn host(&self) -> &'static str {
        "settings"
    }

    fn render(&self, _ctx: &SpecialPageCtx<'_>) -> String {
        let body = r#"<h1>Settings</h1>
<section>
  <h2>About</h2>
  <p class="muted">Settings persist for the current session only. On-disk persistence is coming.</p>
</section>"#;

        page_shell("Settings", body)
    }

    fn handle_action(&self, ctx: &SpecialPageCtx<'_>) {
        let mut key = None;
        let mut value = None;
        for (k, v) in ctx.url.query_pairs() {
            match k.as_ref() {
                "key" => key = Some(v.into_owned()),
                "value" => value = Some(v.into_owned()),
                _ => {}
            }
        }
        if let (Some(k), Some(v)) = (key, value) {
            ctx.config.set(&k, &v);
        }
    }
}
