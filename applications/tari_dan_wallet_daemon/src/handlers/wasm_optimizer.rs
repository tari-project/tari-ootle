// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::io;

use tempfile::tempdir;
use thiserror::Error;
use tokio::{fs::File, io::AsyncWriteExt};
use wasm_opt::{OptimizationError, OptimizationOptions};

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    IO(#[from] io::Error),
    #[error("I/O error: {0}")]
    Optimization(#[from] OptimizationError),
}

/// Optimizes WASM templates
pub struct WasmTemplateOptimizer {}

impl WasmTemplateOptimizer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn optimize(&self, template_binary: &[u8]) -> Result<Vec<u8>, Error> {
        let temp_dir = tempdir()?;

        // create temporary input file
        let input_file_path = temp_dir.path().join("input.wasm");
        let output_file_path = temp_dir.path().join("output.wasm");
        let mut input_file = File::create(input_file_path.as_path()).await?;
        input_file.write_all(template_binary).await?;
        input_file.flush().await?;

        OptimizationOptions::new_optimize_for_size().run(input_file_path, output_file_path.as_path())?;

        let result = tokio::fs::read(output_file_path).await?;

        temp_dir.close()?;

        Ok(result)
    }
}
