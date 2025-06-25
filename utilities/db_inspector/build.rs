//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

fn main() {
    if option_env!("CI").is_some() {
        println!("cargo:warning=CI detected, skipping web UI build");
        return;
    }

    if cfg!(debug_assertions) {
        println!("cargo:warning=The web ui will not be compiled in debug mode.");
        return;
    }

    println!("cargo:rerun-if-changed=web_ui/src");
    println!("cargo:rerun-if-changed=web_ui/package.json");
    if let Err(e) = run() {
        // We never want to fail the build if the build fails for this utility
        println!("cargo:warning=Web UI build failed: {e}");
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    const BIN: &str = "pnpm.cmd";
    #[cfg(not(target_os = "windows"))]
    const BIN: &str = "pnpm";
    run_command(BIN, &["install", "--frozen-lockfile"])?;
    run_command(BIN, &["run", "build"])?;

    Ok(())
}

fn run_command(command: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(command)
        .current_dir("./web_ui")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute command (is {command} installed?): {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Command failed with status: {}: stderr: {}, stdout: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).replace("\n", " "),
            String::from_utf8_lossy(&output.stdout)
        )
        .into());
    }

    Ok(())
}
