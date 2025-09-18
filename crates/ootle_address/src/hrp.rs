//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use bech32::Hrp;
use tari_ootle_common_types::Network;

pub(crate) const HRP_MAINNET: Hrp = Hrp::parse_unchecked("xtr_");
pub(crate) const HRP_LOCALNET: Hrp = Hrp::parse_unchecked("xtr_loc_");
pub(crate) const HRP_ESME: Hrp = Hrp::parse_unchecked("xtr_esm_");
pub(crate) const HRP_IGOR: Hrp = Hrp::parse_unchecked("xtr_igr_");
pub(crate) const HRP_NEXTNET: Hrp = Hrp::parse_unchecked("xtr_nxt_");
pub(crate) const HRP_STAGENET: Hrp = Hrp::parse_unchecked("xtr_stg_");

pub fn hrp_from_network(network: Network) -> Hrp {
    match network {
        Network::MainNet => HRP_MAINNET,
        Network::LocalNet => HRP_LOCALNET,
        Network::Esmeralda => HRP_ESME,
        Network::Igor => HRP_IGOR,
        Network::NextNet => HRP_NEXTNET,
        Network::StageNet => HRP_STAGENET,
    }
}

pub fn network_from_hrp(hrp: &Hrp) -> Option<Network> {
    if *hrp == HRP_MAINNET {
        Some(Network::MainNet)
    } else if *hrp == HRP_LOCALNET {
        Some(Network::LocalNet)
    } else if *hrp == HRP_ESME {
        Some(Network::Esmeralda)
    } else if *hrp == HRP_IGOR {
        Some(Network::Igor)
    } else if *hrp == HRP_NEXTNET {
        Some(Network::NextNet)
    } else if *hrp == HRP_STAGENET {
        Some(Network::StageNet)
    } else {
        None
    }
}
