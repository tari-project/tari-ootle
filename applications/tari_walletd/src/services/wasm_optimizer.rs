// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::io;

use tempfile::tempdir;
use thiserror::Error;
use wasm_opt::{Feature, OptimizationError, OptimizationOptions};

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    IO(#[from] io::Error),
    #[error("Optimization error: {0}")]
    Optimization(#[from] OptimizationError),
    #[error("Invalid file after optimization: {0}")]
    InvalidFile(String),
}

/// Optimizes a WebAssembly (WASM) template binary to reduce its size.
///
/// # Arguments
///
/// * `template_binary` - The original WASM binary to optimize
///
/// # Returns
///
/// A `Result` containing either the optimized WASM binary as a `Vec<u8>` or an `Error`
///
/// # Errors
///
/// Returns an error if:
/// - There are I/O errors when creating temporary files
/// - The optimization process fails
pub async fn optimize_wasm_template(template_binary: &[u8]) -> Result<Vec<u8>, Error> {
    let temp_dir = tempdir()?;

    // create temporary input file
    let input_file_path = temp_dir.path().join("input.wasm");
    let output_file_path = temp_dir.path().join("output.wasm");
    tokio::fs::write(&input_file_path, template_binary).await?;

    OptimizationOptions::new_optimize_for_size()
        .enable_feature(Feature::BulkMemory)
        .enable_feature(Feature::ReferenceTypes)
        .disable_feature(Feature::Simd)
        .disable_feature(Feature::RelaxedSimd)
        .run(input_file_path, output_file_path.as_path())?;

    let result = tokio::fs::read(output_file_path).await?;

    if result.is_empty() {
        return Err(Error::InvalidFile("Empty file".to_string()));
    }

    temp_dir.close()?;

    Ok(result)
}
