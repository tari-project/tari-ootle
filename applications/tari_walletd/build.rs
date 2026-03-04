// Copyright 2021. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{env, fs, process::Command};

type EnvVars = &'static [(&'static str, &'static str)];
const NPM_COMMANDS: &[(&str, &[&str], EnvVars)] = &[
    ("../../bindings", &["install"], &[]),
    ("../../bindings", &["run", "build-dev"], &[]),
    ("../../clients/javascript/wallet_daemon_client", &["install"], &[]),
    ("../../clients/javascript/wallet_daemon_client", &["run", "build"], &[]),
    ("./web_ui", &["clean-dist"], &[]),
    ("./web_ui", &["install"], &[]),
    ("./web_ui", &["run", "build"], {
        match option_env!("TARI_WALLET_ALPHA_FEATURES") {
            Some(feat) => &[("VITE_ALPHA_FEATURES", feat)],
            _ => &[],
        }
    }),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=./web_ui/src");
    println!("cargo:rerun-if-changed=./web_ui/public");

    fs::create_dir_all("./web_ui/dist")?;

    if env::var("CARGO_FEATURE_TS").is_ok() {
        println!("cargo:warning=The web ui will not be built because the tx feature is enabled.");
        return Ok(());
    }
    if env::var("CARGO_FEATURE_WEB_UI").is_err() {
        println!("cargo:warning=The web ui will not be built because the web_ui feature is not enabled.");
        return Ok(());
    }

    if cfg!(debug_assertions) {
        println!("cargo:warning=The web ui will not be compiled in debug mode.");
        return Ok(());
    }

    #[cfg(windows)]
    const NPM: &str = "pnpm.cmd";
    #[cfg(not(windows))]
    const NPM: &str = "pnpm";

    for (target, args, envs) in NPM_COMMANDS {
        match Command::new(NPM)
            .args(*args)
            .envs(envs.iter().copied())
            .current_dir(target)
            .output()
        {
            Ok(output) if !output.status.success() => {
                println!(
                    "cargo:warning='pnpm {}' in {} exited with non-zero status code",
                    args.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(" "),
                    target
                );
                println!("cargo:warning=Status: {}", output.status);
                if !output.stdout.is_empty() {
                    for (i, line) in String::from_utf8_lossy(&output.stdout).lines().enumerate() {
                        println!("cargo:warning=Output {i}: {line}");
                    }
                }
                if !output.stderr.is_empty() {
                    for (i, line) in String::from_utf8_lossy(&output.stderr).lines().enumerate() {
                        println!("cargo:warning=Error {i}: {line}");
                    }
                }
                // Ignore it unless on CI
                continue;
            },
            Err(error) => {
                println!(
                    "cargo:warning='{NPM} run build' error (is {NPM} installed?): {:?}",
                    error
                );
                // If on CI, fail the build. Otherwise, just warn
                if env::var("CI").is_ok() {
                    return Err(Box::new(error));
                }
                println!("cargo:warning=The web ui will not be included in the build!");
                continue;
            },
            _ => {},
        }
    }
    Ok(())
}
