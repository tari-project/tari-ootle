//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::hash_map::DefaultHasher,
    env,
    ffi::{OsStr, OsString},
    fs,
    hash::{Hash, Hasher},
    io,
    io::ErrorKind,
    path::{Path, PathBuf},
    process,
    process::Command,
    thread,
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

    let wasm_name = wasm_name(pkg_dir)?;
    let target_dir = target_dir(pkg_dir, &wasm_name, &target_dir_key(features, &envs))?;

    let mut command = Command::new("cargo");
    command
        .current_dir(pkg_dir)
        .envs(envs)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            // Strip the `name`, `producers` and `target_features` custom sections the wasm
            // toolchain emits. `WasmModule::validate_code` rejects a published template that
            // carries any custom section other than `tari_tdef`, so templates must ship stripped.
            "--config",
            "profile.release.strip=\"symbols\"",
        ]);

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

    let path = target_dir
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(wasm_name)
        .with_extension("wasm");

    let code = fs::read(path).map_err(|e| io::Error::other(format!("Failed to read wasm file: {}", e)))?;
    Ok(WasmModule::from_code(code))
}

fn wasm_name(pkg_dir: &Path) -> io::Result<String> {
    let manifest = Manifest::from_path(pkg_dir.join("Cargo.toml"))
        .map_err(|e| io::Error::other(format!("Failed to read Cargo.toml: {}", e)))?;
    let name = if let Some(Product { name: Some(name), .. }) = manifest.lib {
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
    Ok(name)
}

/// Returns the cargo target directory to compile the template at `pkg_dir` into.
///
/// Under `cargo test` or nextest, the templates a test crate compiles share one target directory per key, below the
/// test crate's own `target`, so the dependencies they have in common compile once. Cargo names a template's wasm
/// after its lib name alone, so two templates with the same name would overwrite each other's artifact there. The
/// first template to claim a name in the shared directory owns it; any other template with that name compiles in a
/// target directory of its own, inside its package.
fn target_dir(pkg_dir: &Path, wasm_name: &str, key: &str) -> io::Result<PathBuf> {
    let pkg_dir = fs::canonicalize(pkg_dir)?;
    let own_target_dir = pkg_dir.join("target").join(key);
    let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR") else {
        return Ok(own_target_dir);
    };

    let shared_target_dir = Path::new(&manifest_dir).join("target").join("templates").join(key);
    let claims_dir = shared_target_dir.join("claims");
    fs::create_dir_all(&claims_dir)?;
    let claim = claims_dir.join(wasm_name);
    let owner = pkg_dir.as_os_str().as_encoded_bytes();
    // The claim is written whole under a name of its own and then linked into place, so a reader never sees a
    // partial claim.
    let pending_claim = claims_dir.join(format!(".{wasm_name}.{}.{:?}", process::id(), thread::current().id()));
    fs::write(&pending_claim, owner)?;
    let linked = fs::hard_link(&pending_claim, &claim);
    fs::remove_file(&pending_claim)?;
    match linked {
        Ok(()) => return Ok(shared_target_dir),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {},
        Err(e) => return Err(e),
    }

    if fs::read(&claim)? == owner {
        Ok(shared_target_dir)
    } else {
        Ok(own_target_dir)
    }
}

/// Builds a stable, per-(features, envs) subdirectory name for a cargo target directory.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_template_with_a_claimed_name_compiles_in_its_own_target_dir() {
        let packages = tempfile::tempdir().unwrap();
        let first = packages.path().join("first");
        let second = packages.path().join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let key = packages.path().file_name().unwrap().to_str().unwrap();
        let shared = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("templates")
            .join(key);

        assert_eq!(target_dir(&first, "same_name", key).unwrap(), shared);
        assert_eq!(target_dir(&first, "same_name", key).unwrap(), shared);
        assert_eq!(
            target_dir(&second, "same_name", key).unwrap(),
            fs::canonicalize(&second).unwrap().join("target").join(key)
        );
        assert_eq!(target_dir(&second, "other_name", key).unwrap(), shared);

        fs::remove_dir_all(shared).unwrap();
    }
}
