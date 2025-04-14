//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

fn main() {
    println!("cargo:rerun-if-changed=web_ui/src");
    if let Err(e) = run() {
        // We never want to fail the build if the build fails for this utility
        println!("cargo:warning=Web UI build failed: {e}");
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    const MOON_BIN: &str = "moon";

    run_command(MOON_BIN, &["db-inspector:install"])?;
    run_command(MOON_BIN, &["db-inspector:build"])?;

    Ok(())
}

fn run_command(command: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute command (is {command} installed?): {e}"))?;

    if !output.status.success() {
        return Err(format!("Command failed with status: {}", output.status).into());
    }

    Ok(())
}
