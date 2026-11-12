//! The Stylo preferences Blitz enables.

use std::sync::Once;

pub(crate) fn init_stylo_prefs() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        style_config::set_pref!("layout.css.flexbox-balance.enabled", true);
        style_config::set_pref!("layout.css.background-clip.text.enabled", true);
        style_config::set_pref!("layout.css.webkit-text-fill-color.enabled", true);
        style_config::set_pref!("layout.css.columns.enabled", true);
        style_config::set_pref!("layout.css.basic-shape-shape.enabled", true);
        style_config::set_pref!("layout.css.tree-counting-functions.enabled", true);
        style_config::set_pref!("layout.css.progress-function.enabled", true);
        style_config::set_pref!("layout.css.font-variations.enabled", true);
        style_config::set_pref!("layout.css.text-indent-keywords.enabled", true);
        style_config::set_pref!("layout.css.word-break.break-word.enabled", true);
        #[cfg(feature = "writing-mode")]
        {
            style_config::set_pref!("layout.css.writing-mode.enabled", true);
            style_config::set_pref!("layout.css.writing-mode.sideways.enabled", true);
        }

        // Properties that were previously enabled by the `layout.unimplemented` pref
        style_config::set_pref!("layout.css.appearance.enabled", true);
        style_config::set_pref!("layout.css.backdrop-filter.enabled", true);
        style_config::set_pref!("layout.css.contain.enabled", true);
        style_config::set_pref!("layout.css.content.alt-text.enabled", true);
        style_config::set_pref!("layout.css.corner-shape.enabled", true);
        style_config::set_pref!("layout.css.counters.enabled", true);
        style_config::set_pref!("layout.css.font-size-adjust.enabled", true);
        style_config::set_pref!("layout.css.forced-color-adjust.enabled", true);
        style_config::set_pref!("layout.css.masking.enabled", true);
        style_config::set_pref!("layout.css.math-properties.enabled", true);
        style_config::set_pref!("layout.css.motion-path.enabled", true);
        style_config::set_pref!("layout.css.overscroll-behavior.enabled", true);
        style_config::set_pref!("layout.css.scroll-driven-animations.enabled", true);
        style_config::set_pref!("layout.css.scroll-behavior.enabled", true);
        style_config::set_pref!("layout.css.scrollbar-styling.enabled", true);
        style_config::set_pref!("layout.css.text-decoration-inset.enabled", true);
        style_config::set_pref!("layout.css.text-overflow.enabled", true);
        style_config::set_pref!("layout.css.text-underline-position.enabled", true);
        style_config::set_pref!("layout.css.view-transitions.enabled", true);
        style_config::set_pref!("layout.css.writing-mode.text-orientation.enabled", true);
        style_config::set_pref!("layout.css.zoom.enabled", true);
        style_config::set_pref!("layout.threads", -1);
    });
}
