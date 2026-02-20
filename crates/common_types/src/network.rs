//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

//  Copyright 2021, The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    convert::TryFrom,
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

/// Represents the available Tari networks. The variants and assigned byte needs to match the L1 network enum.
#[repr(u8)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Copy, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Network {
    MainNet = 0x00,
    StageNet = 0x01,
    NextNet = 0x02,
    LocalNet = 0x10,
    Igor = 0x24,
    #[default]
    Esmeralda = 0x26,
}

impl Network {
    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    pub const fn as_key_str(self) -> &'static str {
        #[allow(clippy::enum_glob_use)]
        use Network::*;
        match self {
            MainNet => "mainnet",
            StageNet => "stagenet",
            NextNet => "nextnet",
            Igor => "igor",
            Esmeralda => "esmeralda",
            LocalNet => "localnet",
        }
    }

    pub const fn is_testnet(&self) -> bool {
        !matches!(self, Network::MainNet)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to parse network: {0}")]
pub struct NetworkParseError(String);

impl NetworkParseError {
    pub fn new<T: Into<String>>(msg: T) -> Self {
        Self(msg.into())
    }
}

impl FromStr for Network {
    type Err = NetworkParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        #[allow(clippy::enum_glob_use)]
        use Network::*;
        match value.to_lowercase().as_str() {
            "mainnet" => Ok(MainNet),
            "nextnet" => Ok(NextNet),
            "stagenet" => Ok(StageNet),
            "localnet" => Ok(LocalNet),
            "igor" => Ok(Igor),
            "esmeralda" | "esme" => Ok(Esmeralda),
            invalid => Err(NetworkParseError(format!("Invalid network string: {}", invalid))),
        }
    }
}

impl TryFrom<u8> for Network {
    type Error = NetworkParseError;

    fn try_from(v: u8) -> Result<Self, NetworkParseError> {
        match v {
            x if x == Network::MainNet as u8 => Ok(Network::MainNet),
            x if x == Network::StageNet as u8 => Ok(Network::StageNet),
            x if x == Network::NextNet as u8 => Ok(Network::NextNet),
            x if x == Network::LocalNet as u8 => Ok(Network::LocalNet),
            x if x == Network::Igor as u8 => Ok(Network::Igor),
            x if x == Network::Esmeralda as u8 => Ok(Network::Esmeralda),
            _ => Err(NetworkParseError(format!("Invalid network byte: {}", v))),
        }
    }
}

impl From<Network> for u8 {
    fn from(network: Network) -> Self {
        network.as_byte()
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_key_str())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn network_bytes() {
        // get networks
        let mainnet = Network::MainNet;
        let stagenet = Network::StageNet;
        let nextnet = Network::NextNet;
        let localnet = Network::LocalNet;
        let igor = Network::Igor;
        let esmeralda = Network::Esmeralda;

        // test .as_byte()
        assert_eq!(mainnet.as_byte(), 0x00_u8);
        assert_eq!(stagenet.as_byte(), 0x01_u8);
        assert_eq!(nextnet.as_byte(), 0x02_u8);
        assert_eq!(localnet.as_byte(), 0x10_u8);
        assert_eq!(igor.as_byte(), 0x24_u8);
        assert_eq!(esmeralda.as_byte(), 0x26_u8);

        // test .as_key_str()
        assert_eq!(mainnet.as_key_str(), "mainnet");
        assert_eq!(stagenet.as_key_str(), "stagenet");
        assert_eq!(nextnet.as_key_str(), "nextnet");
        assert_eq!(localnet.as_key_str(), "localnet");
        assert_eq!(igor.as_key_str(), "igor");
        assert_eq!(esmeralda.as_key_str(), "esmeralda");
    }

    #[test]
    fn network_from_str() {
        // test .from_str()
        assert_eq!(Network::from_str("mainnet").unwrap(), Network::MainNet);
        assert_eq!(Network::from_str("stagenet").unwrap(), Network::StageNet);
        assert_eq!(Network::from_str("nextnet").unwrap(), Network::NextNet);
        assert_eq!(Network::from_str("localnet").unwrap(), Network::LocalNet);
        assert_eq!(Network::from_str("igor").unwrap(), Network::Igor);
        assert_eq!(Network::from_str("esmeralda").unwrap(), Network::Esmeralda);
        assert_eq!(Network::from_str("esme").unwrap(), Network::Esmeralda);
        // catch error case
        let err_network = Network::from_str("invalid network");
        assert!(err_network.is_err());
    }

    #[test]
    fn network_from_byte() {
        const ALL_NETWORKS: [Network; 6] = [
            Network::MainNet,
            Network::StageNet,
            Network::NextNet,
            Network::LocalNet,
            Network::Igor,
            Network::Esmeralda,
        ];

        for network in ALL_NETWORKS {
            assert_eq!(network, Network::try_from(network.as_byte()).unwrap());
        }
    }
}
