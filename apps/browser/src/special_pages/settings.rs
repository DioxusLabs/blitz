use super::{SpecialPage, SpecialPageCtx, body_class_for, page_shell};

pub struct Settings;

impl SpecialPage for Settings {
    fn host(&self) -> &'static str {
        "settings"
    }

    fn render(&self, ctx: &SpecialPageCtx<'_>) -> String {
        let theme = ctx.config.get("theme").unwrap_or_else(|| "light".into());
        let other = if theme == "dark" { "light" } else { "dark" };

        let body = format!(
            r#"<h1>Settings</h1>
<section>
  <h2>Appearance</h2>
  <p>Theme: <strong>{theme}</strong></p>
  <p><a class="btn" href="about:settings/set?key=theme&amp;value={other}">Switch to {other}</a></p>
</section>
<section>
  <h2>About</h2>
  <p class="muted">Settings persist for the current session only. On-disk persistence is coming.</p>
</section>"#
        );

        page_shell("Settings", body_class_for(ctx), &body)
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
