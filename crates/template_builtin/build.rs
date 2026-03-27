//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    env,
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;

const TEMPLATE_BUILTINS: &[&str] = &[
    "templates/account",
    "templates/nft_faucet",
    "templates/faucet",
    "templates/liquidity_pool",
];

fn main() -> anyhow::Result<()> {
    // If templates feature is disabled, do nothing
    if env::var("CARGO_FEATURE_TEMPLATES").is_err() {
        return Ok(());
    }

    // Rebuild templates if abi or lib changes (only if they exist in the build context)
    if Path::new("../template_abi").exists() {
        println!("cargo:rerun-if-changed=../template_abi");
    }
    if Path::new("../template_lib").exists() {
        println!("cargo:rerun-if-changed=../template_lib");
    }
    if Path::new("../tari_bor").exists() {
        println!("cargo:rerun-if-changed=../tari_bor");
    }
    let crate_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let compiled_path = crate_path.join("compiled");
    fs::create_dir_all(&compiled_path).context("Create compiled directory")?;
    for template in TEMPLATE_BUILTINS {
        // we only want to rebuild if a template was added/modified
        println!("cargo:rerun-if-changed={}/src", template);
        println!("cargo:rerun-if-changed={}/Cargo.toml", template);

        let template_path = crate_path.join(template);
        if !template_path.exists() {
            // If the template doesn't exist, skip it. This allows us to have templates that are not included in the
            // build context, without causing build failures.
            continue;
        }

        // compile the template into wasm
        compile_template(&template_path)?;

        // get the path of the wasm executable
        let wasm_name = Path::new(template).file_name().unwrap().to_str().unwrap();
        let wasm_path = get_compiled_wasm_path(&template_path, wasm_name);

        // copy the wasm binary to the root folder of the template, to be included in source control
        let wasm_dest = compiled_path.join(wasm_name).with_extension("wasm");
        println!("cargo:rerun-if-changed={}", wasm_dest.display());
        if wasm_dest.exists() {
            let existing_contents = fs::read(&wasm_dest).context("Read existing_contents")?;
            let dest_contents = fs::read(&wasm_path).context("Read dest_contents")?;
            if existing_contents == dest_contents {
                continue;
            }
        }
        fs::copy(wasm_path, wasm_dest).context("Copy file to dest")?;
    }

    Ok(())
}

fn compile_template<P: AsRef<Path>>(package_dir: P) -> io::Result<()> {
    let args = ["build", "--target", "wasm32-unknown-unknown", "--release"];

    let output = Command::new("cargo")
        .current_dir(package_dir.as_ref())
        .args(args)
        .output()?;

    if !output.status.success() {
        eprintln!("stdout:");
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr:");
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(io::Error::other(format!(
            "Failed to compile package: {:?}",
            package_dir.as_ref(),
        )));
    }

    Ok(())
}

fn get_compiled_wasm_path<P: AsRef<Path>>(template_path: P, wasm_name: &str) -> PathBuf {
    template_path
        .as_ref()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(wasm_name)
        .with_extension("wasm")
}
