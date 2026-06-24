//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::hash_map::DefaultHasher,
    ffi::{OsStr, OsString},
    fs,
    hash::{Hash, Hasher},
    io,
    io::ErrorKind,
    path::Path,
    process::Command,
};

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

    let envs = envs
        .into_iter()
        .map(|(k, v)| (k.as_ref().to_os_string(), v.as_ref().to_os_string()))
        .collect::<Vec<_>>();

    // Compile each (features, envs) combination into a dedicated target directory. Cargo writes the
    // same `release/<name>.wasm` path regardless of feature set, so sharing one target directory lets
    // concurrent test compilations clobber each other: a build that rebuilds for a different
    // fingerprint replaces the artifact while another process is reading it, surfacing as a spurious
    // "No such file or directory" error.
    let target_subdir = Path::new("target").join(target_dir_key(features, &envs));

    let mut command = Command::new("cargo");
    command
        .current_dir(pkg_dir)
        .envs(envs)
        // CARGO_TARGET_DIR is resolved relative to the command's working directory (pkg_dir).
        .env("CARGO_TARGET_DIR", &target_subdir)
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
        .join(&target_subdir)
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(wasm_name)
        .with_extension("wasm");

    let code = fs::read(path).map_err(|e| io::Error::other(format!("Failed to read wasm file: {}", e)))?;
    Ok(WasmModule::from_code(code))
}

/// Builds a stable, per-(features, envs) subdirectory name for the package's cargo target directory.
///
/// Distinct feature sets and environments produce distinct cargo fingerprints yet the same
/// `release/<name>.wasm` output path. Giving each combination its own target directory prevents a
/// concurrent compile from replacing another's artifact while it is being read.
fn target_dir_key(features: &[&str], envs: &[(OsString, OsString)]) -> String {
    if features.is_empty() && envs.is_empty() {
        return "default".to_string();
    }

    let mut hasher = DefaultHasher::new();
    let mut features = features.to_vec();
    features.sort_unstable();
    features.hash(&mut hasher);
    let mut envs = envs.iter().collect::<Vec<_>>();
    envs.sort_unstable();
    envs.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
