//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::Network;

pub fn default_indexer_url(network: Network) -> &'static str {
    match network {
        Network::MainNet => unimplemented!("MainNet indexer URL is not set"),
        Network::LocalNet => "http://localhost:12500",
        Network::StageNet => unimplemented!("StageNet indexer URL is not set"),
        Network::NextNet => unimplemented!("NextNet indexer URL is not set"),
        Network::Igor => unimplemented!("Igor indexer URL is not set"),
        Network::Esmeralda => "http://217.182.93.35:50124",
    }
}
