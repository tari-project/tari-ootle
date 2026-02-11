//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{env, fs, path::Path};

use tari_indexer::ApiDoc;
use utoipa::OpenApi;

// Function to generate and write the file
fn generate_openapi_json<P: AsRef<Path>>(out: P) -> anyhow::Result<()> {
    let openapi = ApiDoc::openapi();
    let json_output = openapi.to_pretty_json()?;
    fs::write(out, json_output)?;
    Ok(())
}

fn main() {
    // Read the first arg as an output path, defaulting to "openapi.json"
    let output = env::args().nth(1).unwrap_or_else(|| "openapi.json".to_string());
    match generate_openapi_json(&output) {
        Ok(_) => println!("Successfully generated {output}"),
        Err(e) => eprintln!("Error generating openapi.json: {}", e),
    }
}
