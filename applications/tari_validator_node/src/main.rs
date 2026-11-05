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

mod cli;

use std::{fs, fs::OpenOptions, io::Write, panic, time::SystemTime};

use clap::Parser;
use log::*;
use tari_ootle_app_utilities::{configuration::load_configuration, keypair::setup_keypair_prompt};
use tari_shutdown::Shutdown;
use tari_validator_node::{ApplicationConfig, node, run_validator_node};

use crate::cli::Cli;

const LOG_TARGET: &str = "tari::validator_node::app";

#[cfg(feature = "tokio_debug")]
const DEBUG_PORT: u16 = console_subscriber::Server::DEFAULT_PORT;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    install_panic_hook(cfg!(panic = "abort"));

    let cli = Cli::parse();

    let config_path = cli.common.config_path();
    let cfg = load_configuration(config_path, true, &cli, Some(cli.network()))?;
    let mut config = ApplicationConfig::load_from(&cfg)?;
    // An explicit oracle config file given on the CLI takes precedence over both `config_file` and `config_json`.
    if let Some(path) = cli.epoch_oracle_config.clone() {
        config.epoch_oracle.configured.set_config_file_override(path);
    }

    // Remove the pid file if it exists
    let _file = fs::remove_file(config.common.base_path.join("pid")).inspect_err(|e| {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(
                target: LOG_TARGET,
                "Failed to remove existing pid file: {}", e
            );
        }
    });
    let _logging = tari_logging::init(
        &config.validator_node.logging,
        config.common.base_path(),
        extra_log_layers(&cli),
    )?;
    let log4rs_config = cli.common.log_config_path("validator");
    if log4rs_config.exists() {
        warn!(
            target: LOG_TARGET,
            "{} is not read: logging is configured in the [validator_node.logging] section of {}",
            log4rs_config.display(),
            cli.common.config_path().display(),
        );
    }

    match cli.command {
        Some(cli::Subcommand::CompactDb) => {
            let timer = std::time::Instant::now();
            tari_validator_node::consensus::spec::ValidatorNodeStateStore::compact_all(
                &config.validator_node.state_db_path,
            )?;

            info!(
                target: LOG_TARGET,
                "Compacted state database in {:.2?}",
                timer.elapsed()
            );
            return Ok(());
        },
        Some(cli::Subcommand::Start) | None => {
            let shutdown = Shutdown::new();
            let keypair = setup_keypair_prompt(
                &config.validator_node.identity_file,
                !config.validator_node.dont_create_id,
            )?;

            run_validator_node(keypair, config, shutdown).await?;
            info!(target: LOG_TARGET, "Validator node shutdown successfully");
        },
        Some(cli::Subcommand::GenerateIdentity) => {
            let keypair = setup_keypair_prompt(
                &config.validator_node.identity_file,
                !config.validator_node.dont_create_id,
            )?;

            info!(
                target: LOG_TARGET,
                "Generated identity with public key: {}",
                keypair.public_key()
            );
        },
    }

    let metrics = tokio::runtime::Handle::current().metrics();
    info!(
        target: LOG_TARGET,
        "Tokio runtime metrics: num_alive_tasks={}, num_workers={}, global_queue_depth={}",
        metrics.num_alive_tasks(),
        metrics.num_workers(),
        metrics.global_queue_depth(),
    );

    Ok(())
}

/// Makes a panic in any thread crash the node: the hook logs and records the panic, prints the default message and
/// notifies the node to shut down.
///
/// A process that aborts on panic exits as soon as the hook returns, without running the destructor that drains the
/// log queues, so `abort_follows` has the hook drain them itself. Logging is closed from then on.
fn install_panic_hook(abort_follows: bool) {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        handle_panic(info);
        default_hook(info);
        node::trigger_panic_notifier();
        if abort_follows {
            tari_logging::flush_and_close();
        }
    }));
}

#[cfg(feature = "tokio_debug")]
fn extra_log_layers(cli: &Cli) -> Vec<tari_logging::BoxedLayer> {
    vec![Box::new(
        console_subscriber::ConsoleLayer::builder()
            .server_addr((
                std::net::Ipv4Addr::LOCALHOST,
                cli.tokio_console_port.unwrap_or(DEBUG_PORT),
            ))
            .spawn::<tari_logging::Registry>(),
    )]
}

#[cfg(not(feature = "tokio_debug"))]
fn extra_log_layers(_cli: &Cli) -> Vec<tari_logging::BoxedLayer> {
    Vec::new()
}

fn handle_panic(panic_info: &panic::PanicHookInfo) {
    fn format_current_time() -> String {
        let now = SystemTime::now();
        ::time::OffsetDateTime::from(now)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|e| format!("format time fail: {e}"))
    }

    let location = panic_info
        .location()
        .map(|loc| format!("file: '{}', line: {}", loc.file(), loc.line()))
        .unwrap_or_else(|| "unknown location".to_string());

    let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
        *s
    } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
        s.as_str()
    } else {
        "Unknown panic message"
    };

    error!(target: LOG_TARGET, "Panic occurred at {location}: {message}");
    tari_validator_node::diagnostics::record_panic(&location, message);

    if let Err(err) = OpenOptions::new()
        .append(true)
        .create(true)
        .open("ootle-node-panic.log")
        .and_then(|mut file| {
            file.write_all(b"---\n")?;
            file.write_all(format!("Timestamp: {}\n", format_current_time()).as_bytes())?;
            file.write_all(format!("Panic at {}: {}\n", location, message).as_bytes())?;
            file.write_all(b"---\n")
        })
    {
        warn!(target: LOG_TARGET, "Failed to write panic log file: {}", err);
    }
}

#[cfg(test)]
mod tests {
    use std::{env, process::Command};

    use tari_validator_node::logging::default_logging_config;

    use super::*;

    const CHILD_DIR_VAR: &str = "OOTLE_VN_PANIC_TEST_DIR";
    const PANIC_MESSAGE: &str = "panic-flush-test-4f1c";
    const BACKLOG_BYTES: usize = 32 * 1024 * 1024;

    /// Under `panic = 'abort'` the process ends as the hook returns, so the panic line reaches the log file only if
    /// the hook drains the queue. The child panics in a process that aborts right after the hook.
    #[test]
    fn panic_message_reaches_the_log_file_when_the_process_aborts() {
        let dir = tempfile::tempdir().unwrap();
        let status = Command::new(env::current_exe().unwrap())
            .args([
                "tests::panicking_child",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_DIR_VAR, dir.path())
            .status()
            .unwrap();
        assert!(!status.success(), "the child must abort");

        let log = fs::read_to_string(dir.path().join("log/validator-node/ootle.log")).unwrap_or_default();
        assert!(
            log.contains(PANIC_MESSAGE),
            "panic message missing from the {} bytes of ootle.log",
            log.len()
        );
    }

    #[test]
    #[ignore = "run as a child process by panic_message_reaches_the_log_file_when_the_process_aborts"]
    fn panicking_child() {
        let Some(dir) = env::var_os(CHILD_DIR_VAR) else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        // The panic hook also writes a panic file into the working directory.
        env::set_current_dir(&dir).unwrap();
        let _logging = tari_logging::init(&default_logging_config(), &dir, Vec::new()).unwrap();
        // A line large enough to keep the writer busy while the hook runs, so the panic line is still queued behind it
        // when the hook returns.
        debug!(target: LOG_TARGET, "{}", "x".repeat(BACKLOG_BYTES));

        install_panic_hook(true);
        let hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            hook(info);
            std::process::abort();
        }));
        panic!("{PANIC_MESSAGE}");
    }
}
