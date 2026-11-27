//! User-agent stylesheets.

use std::sync::LazyLock;

use crate::DEFAULT_CSS;
use crate::url::DocumentUrl;
use style::media_queries::MediaList;
use style::servo_arc::Arc as ServoArc;
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet};
use style::{context::QuirksMode, shared_lock::SharedRwLock};

use super::prefs::init_stylo_prefs;

/// A user-agent stylesheet, parsed with a read-only lock so that it can be shared between documents.
///
/// Documents with the same user-agent stylesheets (in the same order) also share Stylo's
/// cached user-agent cascade data. `@import` rules are ignored.
#[derive(Clone, Debug, PartialEq)]
pub struct UserAgentStylesheet(pub(crate) DocumentStyleSheet);

impl UserAgentStylesheet {
    pub fn parse(css: &str) -> Self {
        // Pref-gated UA declarations must be enabled before parsing.
        init_stylo_prefs();

        let lock = SharedRwLock::read_only();
        let data = Stylesheet::from_str(
            css,
            DocumentUrl::default().url_extra_data(),
            Origin::UserAgent,
            ServoArc::new(lock.wrap(MediaList::empty())),
            lock,
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::No,
        );
        Self(DocumentStyleSheet(ServoArc::new(data)))
    }

    /// The default user-agent stylesheet ([`DEFAULT_CSS`]), parsed once.
    pub fn default_sheet() -> Self {
        static SHEET: LazyLock<UserAgentStylesheet> =
            LazyLock::new(|| UserAgentStylesheet::parse(DEFAULT_CSS));
        SHEET.clone()
    }

    pub fn sheet(&self) -> &DocumentStyleSheet {
        &self.0
    }
}
