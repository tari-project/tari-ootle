//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{env, fs, io::Write, path::Path};

use tari_indexer::ApiDoc;
use utoipa::OpenApi;

// Function to generate and write the file
fn generate_openapi_json<P: AsRef<Path>>(out: P) -> anyhow::Result<()> {
    let openapi = ApiDoc::openapi();
    let mut file = fs::File::create(&out)?;
    serde_json::to_writer_pretty(&mut file, &openapi)?;
    // Add a newline at the end of the file for POSIX compliance
    file.write_all(b"\n")?;
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
