//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{ffi::OsStr, fs, io, io::ErrorKind, path::Path, process::Command};

use cargo_toml::{Manifest, Product};
use tari_engine::wasm::WasmModule;

pub fn compile_template<P>(package_dir: P, features: &[&str]) -> io::Result<WasmModule>
where P: AsRef<Path> {
    compile_template_internal(package_dir, features, None::<(String, String)>)
}

pub fn compile_template_with_envs<P, TEnvs, K, V>(
    package_dir: P,
    features: &[&str],
    envs: TEnvs,
) -> io::Result<WasmModule>
where
    P: AsRef<Path>,
    TEnvs: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    compile_template_internal(package_dir, features, envs)
}

fn compile_template_internal<P, TEnvs, K, V>(package_dir: P, features: &[&str], envs: TEnvs) -> io::Result<WasmModule>
where
    P: AsRef<Path>,
    TEnvs: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let pkg_dir = package_dir.as_ref();
    if !pkg_dir.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("Package directory not found: {}", pkg_dir.display()),
        ));
    }

    let mut command = Command::new("cargo");
    command
        .current_dir(pkg_dir)
        .envs(envs)
        .args(["build", "--target", "wasm32-unknown-unknown", "--release"]);

    if !features.is_empty() {
        command.arg("--features");
        command.args(features.iter().map(ToString::to_string));
    }

    let output = command.output()?;
    if !output.status.success() {
        eprintln!("stdout:");
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr:");
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(io::Error::other(format!(
            "Failed to compile package: {}",
            pkg_dir.display()
        )));
    }

    // resolve wasm name
    let manifest = Manifest::from_path(pkg_dir.join("Cargo.toml"))
        .map_err(|e| io::Error::other(format!("Failed to read Cargo.toml: {}", e)))?;
    let wasm_name = if let Some(Product { name: Some(name), .. }) = manifest.lib {
        // lib name
        name
    } else if let Some(pkg) = manifest.package {
        // package name
        pkg.name.replace('-', "_")
    } else {
        // file name
        pkg_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
            .replace('-', "_")
    };

    // path of the wasm executable
    let path = pkg_dir
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(wasm_name)
        .with_extension("wasm");

    let code = fs::read(path).map_err(|e| io::Error::other(format!("Failed to read wasm file: {}", e)))?;
    Ok(WasmModule::from_code(code))
}
