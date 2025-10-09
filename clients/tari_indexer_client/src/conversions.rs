//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::models::WalletUtxoUpdate;

use crate::protobuf;

impl From<&WalletUtxoUpdate> for protobuf::WalletUtxoUpdate {
    fn from(value: &WalletUtxoUpdate) -> Self {
        match value {
            WalletUtxoUpdate::Unspent(unspent) => protobuf::WalletUtxoUpdate::Unspent(protobuf::UtxoUnspent {
                tag: unspent.tag.value(),
                public_nonce: unspent.public_nonce.to_vec(),
            }),
            WalletUtxoUpdate::Spent(spent) => protobuf::WalletUtxoUpdate::Spent(protobuf::UtxoSpent {
                id: spent.id.as_bytes().to_vec(),
                version: spent.version,
            }),
            WalletUtxoUpdate::Burnt(burnt) => protobuf::WalletUtxoUpdate::Burnt(protobuf::UtxoBurnt {
                id: burnt.id.as_bytes().to_vec(),
                version: burnt.version,
            }),
        }
    }
}
