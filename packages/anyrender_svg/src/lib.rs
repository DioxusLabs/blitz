// Copyright 2023 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Render an SVG document to a Vello [`Scene`](vello::Scene).
//!
//! This currently lacks support for a [number of important](crate#unsupported-features) SVG features.
//!
//! This is also intended to be the preferred integration between Vello and [usvg], so [consider
//! contributing](https://github.com/linebender/vello_svg) if you need a feature which is missing.
//!
//! This crate also re-exports [`usvg`] and [`vello`], so you can easily use the specific versions that are compatible with Vello SVG.
//!
//! # Unsupported features
//!
//! Missing features include:
//! - text
//! - group opacity
//! - mix-blend-modes
//! - clipping
//! - masking
//! - filter effects
//! - group background
//! - path shape-rendering
//! - patterns

// LINEBENDER LINT SET - lib.rs - v1
// See https://linebender.org/wiki/canonical-lints/
// These lints aren't included in Cargo.toml because they
// shouldn't apply to examples and tests
#![warn(unused_crate_dependencies)]
#![warn(clippy::print_stdout, clippy::print_stderr)]
// END LINEBENDER LINT SET
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
// The following lints are part of the Linebender standard set,
// but resolving them has been deferred for now.
// Feel free to send a PR that solves one or more of these.
#![allow(missing_docs, clippy::shadow_unrelated, clippy::missing_errors_doc)]
#![cfg_attr(test, allow(unused_crate_dependencies))] // Some dev dependencies are only used in tests

mod render;

mod error;
pub use error::Error;

pub mod util;

/// Re-export usvg.
pub use usvg;

use anyrender::Scene;
use kurbo::Affine;

/// Append an SVG to a vello [`Scene`](vello::Scene), with default error handling.
///
/// This will draw a red box over (some) unsupported elements.
pub fn append<S: Scene>(scene: &mut S, svg: &str, transform: Affine) -> Result<(), Error> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opt)?;
    append_tree(scene, &tree, transform);
    Ok(())
}

/// Append an SVG to a vello [`Scene`](vello::Scene), with user-provided error handling logic.
///
/// See the [module level documentation](crate#unsupported-features) for a list of some unsupported svg features
pub fn append_with<S: Scene, F: FnMut(&mut S, &usvg::Node)>(
    scene: &mut S,
    svg: &str,
    transform: Affine,
    error_handler: &mut F,
) -> Result<(), Error> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opt)?;
    append_tree_with(scene, &tree, transform, error_handler);
    Ok(())
}

/// Append an [`usvg::Tree`] to a vello [`Scene`](vello::Scene), with default error handling.
///
/// This will draw a red box over (some) unsupported elements.
pub fn append_tree<S: Scene>(scene: &mut S, svg: &usvg::Tree, transform: Affine) {
    append_tree_with(scene, svg, transform, &mut util::default_error_handler);
}

/// Append an [`usvg::Tree`] to a vello [`Scene`](vello::Scene), with user-provided error handling logic.
///
/// See the [module level documentation](crate#unsupported-features) for a list of some unsupported svg features
pub fn append_tree_with<S: Scene, F: FnMut(&mut S, &usvg::Node)>(
    scene: &mut S,
    svg: &usvg::Tree,
    transform: Affine,
    error_handler: &mut F,
) {
    render::render_group(scene, svg.root(), transform, error_handler);
}

#[cfg(test)]
mod tests {
    // CI will fail unless cargo nextest can execute at least one test per workspace.
    // Delete this dummy test once we have an actual real test.
    #[test]
    fn dummy_test_until_we_have_a_real_test() {}
}
