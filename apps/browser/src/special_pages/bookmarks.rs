use super::{SpecialPage, SpecialPageCtx, body_class_for, page_shell};

pub struct Bookmarks;

impl SpecialPage for Bookmarks {
    fn host(&self) -> &'static str {
        "bookmarks"
    }

    fn render(&self, ctx: &SpecialPageCtx<'_>) -> String {
        let body = r#"<h1>Bookmarks</h1>
<section>
  <p class="muted">Coming soon. Bookmark management will live here.</p>
</section>"#;

        page_shell("Bookmarks", body_class_for(ctx), body)
    }
}
