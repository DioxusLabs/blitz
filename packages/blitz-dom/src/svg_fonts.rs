//! Web fonts for inline SVG text.
//!
//! usvg lays out `<text>` against its own font database, which by default holds only the system
//! fonts. In a browser, SVG text uses the page's `@font-face` fonts like any other text, so each
//! document keeps a copy of the system database with its web fonts added under their CSS family
//! names, and inline SVG is parsed against that copy.

use std::sync::Arc;

use blitz_traits::net::Bytes;
use markup5ever::{local_name, ns};
use usvg::fontdb;

use crate::layout::damage::ALL_DAMAGE;
use crate::net::FontFaceOverrides;
use crate::node::NodeFlags;
use crate::{BaseDocument, util::FONT_DB};

impl BaseDocument {
    /// The font database inline SVG on this document is parsed against.
    pub(crate) fn svg_fontdb(&self) -> Arc<fontdb::Database> {
        self.svg_fontdb
            .clone()
            .unwrap_or_else(|| Arc::clone(&*FONT_DB))
    }

    /// Add a loaded `@font-face` font to the inline SVG font database under its CSS family name,
    /// and rebuild the inline SVGs that may draw text with it.
    ///
    /// usvg picks a face by family, weight and style alone, so it cannot honour `unicode-range`:
    /// a Google Fonts family arrives as one file per script subset, and a Cyrillic or Vietnamese
    /// subset chosen for Latin text falls back to a system font. Only faces whose range covers
    /// Basic Latin letters are registered, and never under the file's own family name, which
    /// would compete with the CSS alias.
    pub(crate) fn register_svg_font(&mut self, bytes: Bytes, overrides: &FontFaceOverrides) {
        let Some(family) = &overrides.family_name else {
            return;
        };
        let covers_latin = overrides.unicode_range.as_ref().is_none_or(|ranges| {
            ranges
                .iter()
                .any(|&(start, end)| start <= 'a' as u32 && end >= 'a' as u32)
        });
        if !covers_latin {
            return;
        }
        let db = Arc::make_mut(self.svg_fontdb.get_or_insert_with(|| Arc::clone(&*FONT_DB)));
        for id in db.load_font_source(fontdb::Source::Binary(Arc::new(bytes))) {
            let Some(mut info) = db.face(id).cloned() else {
                continue;
            };
            db.remove_face(id);
            info.families = vec![(family.clone(), fontdb::Language::English_UnitedStates)];
            if let Some(weight) = overrides.weight {
                info.weight = fontdb::Weight(weight as u16);
            }
            if let Some(style) = overrides.style {
                info.style = match style {
                    parley::fontique::FontStyle::Normal => fontdb::Style::Normal,
                    parley::fontique::FontStyle::Italic => fontdb::Style::Italic,
                    parley::fontique::FontStyle::Oblique(_) => fontdb::Style::Oblique,
                };
            }
            db.push_face_info(info);
        }
        self.damage_inline_svgs_with_text();
    }

    /// Rebuild the outermost inline `<svg>` elements that contain `<text>`.
    fn damage_inline_svgs_with_text(&mut self) {
        let roots: Vec<_> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.flags.contains(NodeFlags::IS_IN_DOCUMENT))
            .filter(|(_, node)| {
                node.element_data()
                    .is_some_and(|el| el.name.ns == ns!(svg) && el.name.local == local_name!("svg"))
                    && !node.parent.is_some_and(|parent| {
                        self.nodes[parent]
                            .element_data()
                            .is_some_and(|el| el.name.ns == ns!(svg))
                    })
            })
            .map(|(id, _)| id)
            .filter(|&id| self.subtree_has_svg_text(id))
            .collect();
        for id in roots {
            let node = &mut self.nodes[id];
            node.insert_damage(ALL_DAMAGE);
            node.mark_ancestors_dirty();
        }
    }

    fn subtree_has_svg_text(&self, id: crate::NodeId) -> bool {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            if node
                .element_data()
                .is_some_and(|el| el.name.local == local_name!("text"))
            {
                return true;
            }
            stack.extend(node.children.iter().copied());
        }
        false
    }
}
