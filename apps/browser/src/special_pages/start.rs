use super::{SpecialPage, SpecialPageCtx, page_shell};

pub struct Start;

const LOGO_B64: &str = include_str!("blitz_logo.b64");

impl SpecialPage for Start {
    fn host(&self) -> &'static str {
        "newtab"
    }

    fn render(&self, ctx: &SpecialPageCtx<'_>) -> String {
        let theme = ctx.config.get("theme").unwrap_or_else(|| "light".into());
        let body_class = if theme == "dark" { "dark" } else { "" };

        let body = format!(
            r#"<div style="display:flex;flex-direction:column;align-items:center;justify-content:center;min-height:60vh;text-align:center;">
  <img src="data:image/png;base64,{LOGO_B64}" alt="Blitz" style="width:160px;height:160px;">
  <h1 style="margin-top:24px;">Blitz</h1>
  <div style="margin-top:32px;display:flex;gap:12px;">
    <a class="btn" href="about:settings">Settings</a>
    <a class="btn" href="about:history">History</a>
    <a class="btn" href="about:bookmarks">Bookmarks</a>
  </div>
</div>"#
        );

        page_shell("New Tab", body_class, &body)
    }
}
