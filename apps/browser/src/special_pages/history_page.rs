use dioxus_native::prelude::*;

use super::{SpecialPage, SpecialPageCtx, page_shell};
use crate::history::HistoryStoreExt;

pub struct HistoryPage;

impl SpecialPage for HistoryPage {
    fn host(&self) -> &'static str {
        "history"
    }

    fn render(&self, ctx: &SpecialPageCtx<'_>) -> String {
        let theme = ctx.config.get("theme").unwrap_or_else(|| "light".into());
        let body_class = if theme == "dark" { "dark" } else { "" };

        let urls = ctx.history.urls();
        let entries = urls.read();

        let list = if entries.is_empty() {
            "<p class=\"muted\">No history yet.</p>".to_string()
        } else {
            let items: String = entries
                .iter()
                .map(|req| {
                    let url = req.url.as_str();
                    let escaped = html_escape(url);
                    format!("<li><a href=\"{escaped}\">{escaped}</a></li>")
                })
                .collect();
            format!("<ul>{items}</ul>")
        };

        let body = format!(
            r#"<h1>History</h1>
<section>
  <h2>Current tab</h2>
  {list}
</section>
<section>
  <h2>Coming soon</h2>
  <p class="muted">Cross-tab history is not yet implemented. This page currently lists only the active tab's history.</p>
</section>"#
        );

        page_shell("History", body_class, &body)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
