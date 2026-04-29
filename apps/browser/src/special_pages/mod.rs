use std::sync::Arc;

use blitz_traits::net::Url;

use crate::config::ConfigStore;
use crate::history::{History, SyncStore};

mod bookmarks;
mod history_page;
mod settings;
mod start;

pub struct SpecialPageCtx<'a> {
    pub url: &'a Url,
    pub history: SyncStore<History>,
    pub config: Arc<ConfigStore>,
}

pub trait SpecialPage: Send + Sync {
    fn host(&self) -> &'static str;
    fn render(&self, ctx: &SpecialPageCtx<'_>) -> String;
    fn handle_action(&self, _ctx: &SpecialPageCtx<'_>) {}
}

pub fn registry() -> &'static [&'static dyn SpecialPage] {
    &[
        &start::Start,
        &settings::Settings,
        &history_page::HistoryPage,
        &bookmarks::Bookmarks,
    ]
}

pub fn lookup(url: &Url) -> Option<&'static dyn SpecialPage> {
    if url.scheme() != "about" {
        return None;
    }
    let host = url.path().split('/').next().unwrap_or("");
    registry().iter().copied().find(|p| p.host() == host)
}

pub fn dispatch(ctx: &SpecialPageCtx<'_>) -> Option<String> {
    let page = lookup(ctx.url)?;
    let path = ctx.url.path();
    if path.contains('/') {
        page.handle_action(ctx);
    }
    Some(page.render(ctx))
}

const SHARED_STYLES: &str = r#"
:root {
    --bg: #f7f7f8;
    --fg: #1a1a1a;
    --muted: #6b7280;
    --accent: #01633f;
    --border: #e5e7eb;
    --card: #ffffff;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14161a;
    --fg: #e6e6e6;
    --muted: #9aa0a6;
    --accent: #01633f;
    --border: #2a2d33;
    --card: #1c1f24;
  }
}
html, body { margin: 0; padding: 0; }
body {
    font-family: -apple-system, "Segoe UI", Helvetica, Arial, sans-serif;
    background: var(--bg);
    color: var(--fg);
    padding: 32px 48px;
    line-height: 1.5;
}
h1 { font-size: 28px; margin: 0 0 12px; }
h2 { font-size: 18px; margin: 24px 0 8px; color: var(--muted); font-weight: 600; }
section {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px 20px;
    margin: 12px 0;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
.btn {
    display: inline-block;
    padding: 6px 14px;
    background: var(--accent);
    color: white;
    border-radius: 6px;
    font-size: 14px;
}
.btn:hover { text-decoration: none; opacity: 0.9; }
.muted { color: var(--muted); font-size: 14px; }
ul { padding-left: 20px; }
li { margin: 4px 0; }
"#;

pub fn page_shell(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{SHARED_STYLES}</style>
</head>
<body>
{body}
</body>
</html>"#
    )
}
