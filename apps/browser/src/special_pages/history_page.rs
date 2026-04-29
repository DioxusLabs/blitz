use dioxus_native::prelude::*;

use super::{SpecialPage, SpecialPageCtx, body_class_for, page_shell};
use crate::history::HistoryStoreExt;

pub struct HistoryPage;

impl SpecialPage for HistoryPage {
    fn host(&self) -> &'static str {
        "history"
    }

    fn render(&self, ctx: &SpecialPageCtx<'_>) -> String {
        let urls_store = ctx.history.urls();
        let urls = urls_store.read();
        let titles_store = ctx.history.titles();
        let titles = titles_store.read();

        let has_real_entries = urls.iter().any(|req| req.url.scheme() != "about");
        let list = if !has_real_entries {
            "<p class=\"muted\">No history yet.</p>".to_string()
        } else {
            let items: String = urls
                .iter()
                .zip(titles.iter())
                .filter(|(req, _)| req.url.scheme() != "about")
                .map(|(req, title)| {
                    let url = req.url.as_str();
                    let escaped_url = html_escape(url);
                    let display = title
                        .as_deref()
                        .filter(|t| !t.trim().is_empty())
                        .map(html_escape)
                        .unwrap_or_else(|| escaped_url.clone());
                    format!("<li><a href=\"{escaped_url}\">{display}</a></li>")
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

        page_shell("History", body_class_for(ctx), &body)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
