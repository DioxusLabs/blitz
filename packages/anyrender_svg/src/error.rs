// Copyright 2023 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use thiserror::Error;

/// Triggered when there is an issue parsing user input.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Error parsing svg: {0}")]
    Svg(#[from] usvg::Error),
}
