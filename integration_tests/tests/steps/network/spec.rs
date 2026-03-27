//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSpec {
    #[serde(default = "default_base_node")]
    pub base_node: NodeSpec,
    #[serde(default = "default_minotari_wallet")]
    pub minotari_wallet: NodeSpec,
    #[serde(default = "default_miner")]
    pub miner: NodeSpec,
    #[serde(default = "default_validator")]
    pub validators: Vec<ValidatorSpec>,
    #[serde(default = "default_walletd")]
    pub walletds: Vec<WalletdSpec>,
    #[serde(default = "default_indexer")]
    pub indexer: NodeSpec,
}

impl Default for NetworkSpec {
    fn default() -> Self {
        Self {
            base_node: default_base_node(),
            minotari_wallet: default_minotari_wallet(),
            miner: default_miner(),
            validators: default_validator(),
            walletds: default_walletd(),
            indexer: default_indexer(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalletdSpec {
    #[serde(flatten)]
    pub node: NodeSpec,
    #[serde(default)]
    pub with_account: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidatorSpec {
    #[serde(flatten)]
    pub node: NodeSpec,
    #[serde(default)]
    pub fee_claim_account: Option<String>,
}

impl ValidatorSpec {
    pub fn name(&self) -> &str {
        &self.node.name
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeSpec {
    pub name: String,
}

fn default_base_node() -> NodeSpec {
    NodeSpec {
        name: "BASE_NODE".to_string(),
    }
}

fn default_miner() -> NodeSpec {
    NodeSpec {
        name: "MINER".to_string(),
    }
}

fn default_indexer() -> NodeSpec {
    NodeSpec {
        name: "INDEXER".to_string(),
    }
}

fn default_minotari_wallet() -> NodeSpec {
    NodeSpec {
        name: "MINOTARI_WALLET".to_string(),
    }
}

fn default_validator() -> Vec<ValidatorSpec> {
    vec![ValidatorSpec {
        node: NodeSpec {
            name: "VALIDATOR".to_string(),
        },
        fee_claim_account: None,
    }]
}

fn default_walletd() -> Vec<WalletdSpec> {
    vec![WalletdSpec {
        node: NodeSpec {
            name: "WALLETD".to_string(),
        },
        with_account: None,
    }]
}
