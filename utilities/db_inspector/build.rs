//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

fn main() {
    if option_env!("CI").is_some() {
        println!("cargo:warning=CI detected, skipping web UI build");
        return;
    }

    println!("cargo:rerun-if-changed=web_ui/src");
    println!("cargo:rerun-if-changed=web_ui/package.json");
    println!("cargo:rerun-if-changed=web_ui/moon.yml");
    if let Err(e) = run() {
        // We never want to fail the build if the build fails for this utility
        println!("cargo:warning=Web UI build failed: {e}");
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    const MOON_BIN: &str = "moon.cmd";
    #[cfg(not(target_os = "windows"))]
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
