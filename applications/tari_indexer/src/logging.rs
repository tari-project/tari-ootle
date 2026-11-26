//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, path::PathBuf};

use tari_logging::{ConsoleFormat, LogFileConfig, LoggingConfig};

/// The indexer's sinks: one file for the indexer and the Ootle crates it runs, and one each for the networking
/// service and the libp2p crates beneath it.
pub fn default_logging_config() -> LoggingConfig {
    let files = [
        (
            "indexer",
            LogFileConfig::new(directives(&[
                "tari::application=debug",
                "tari::indexer=debug",
                "tari::logging=warn",
                "tari::ootle=debug",
                "tower_http=debug",
            ]))
            .with_max_file_size_mb(200)
            .with_max_rotated_files(4),
        ),
        (
            "network",
            LogFileConfig::new(directives(&["tari::networking=debug", "tari::ootle::p2p=debug"])),
        ),
        (
            "libp2p",
            LogFileConfig::new(directives(&[
                // Takes the warnings of the libp2p crates listed nowhere else.
                "libp2p=warn",
                "libp2p_core=info",
                "libp2p_gossipsub=info",
                "libp2p_identify=debug",
                "libp2p_mdns=debug",
                "libp2p_messaging=debug",
                "libp2p_noise=debug",
                "libp2p_ping=debug",
                "libp2p_quic=debug",
                "libp2p_rendezvous=debug",
                "libp2p_swarm=debug",
                "libp2p_tcp=debug",
                "quinn=info",
                "hickory_proto=info",
                "multistream_select=info",
            ]))
            .with_max_file_size_mb(10),
        ),
    ];

    LoggingConfig {
        directory: PathBuf::from("log/indexer"),
        console: directives(&[
            "tari::application=info",
            "tari::indexer=info",
            "tari::logging=warn",
            "tari::ootle=info",
        ]),
        console_format: ConsoleFormat::Text,
        files: files
            .into_iter()
            .map(|(name, file)| (name.to_string(), file))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn directives(directives: &[&str]) -> String {
    directives.join(",")
}

#[cfg(test)]
mod tests {
    use tari_logging::build;

    use super::*;

    #[test]
    fn a_configured_key_changes_only_that_key() {
        use tari_common::DefaultConfigLoader;

        use crate::config::IndexerConfig;

        let config = config::Config::builder()
            .set_override("indexer.logging.files.indexer.filter", "tari::indexer=info")
            .unwrap()
            .set_override("indexer.logging.console_format", "json")
            .unwrap()
            .build()
            .unwrap();
        let logging = IndexerConfig::load_from(&config).unwrap().logging;
        let defaults = default_logging_config();

        let indexer = &logging.files["indexer"];
        assert_eq!(indexer.filter, "tari::indexer=info");
        assert_eq!(indexer.max_file_size_mb, defaults.files["indexer"].max_file_size_mb);
        assert_eq!(indexer.max_rotated_files, defaults.files["indexer"].max_rotated_files);
        assert_eq!(logging.console_format, ConsoleFormat::Json);
        assert_eq!(logging.console, defaults.console);
    }

    #[test]
    fn default_filters_parse() {
        let dir = tempfile::tempdir().unwrap();
        let config = default_logging_config();
        let (_subscriber, _writers) = build(&config, &config.console, dir.path(), Vec::new()).unwrap();
        for name in config.files.keys() {
            assert!(dir.path().join("log/indexer").join(format!("{name}.log")).exists());
        }
    }

    #[test]
    fn networking_service_lines_reach_the_network_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = default_logging_config();
        let (subscriber, writers) = build(&config, &config.console, dir.path(), Vec::new()).unwrap();
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "tari::networking::service::worker", "discovered peer via rendezvous");
            tracing::warn!(target: "libp2p_relay::priv_client", "relay reservation failed");
        });
        drop(writers);

        let network = std::fs::read_to_string(dir.path().join("log/indexer/network.log")).unwrap();
        assert!(network.contains("discovered peer via rendezvous"), "{network}");
        let libp2p = std::fs::read_to_string(dir.path().join("log/indexer/libp2p.log")).unwrap();
        assert!(libp2p.contains("relay reservation failed"), "{libp2p}");
    }
}
