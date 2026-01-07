//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_node_components::blocks::BlockHeader;
use tari_ootle_common_types::{Epoch, NodeAddressable};
use tari_ootle_storage::global::{BlockHeaderModel, GlobalDb};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;

pub trait BaseLayerBlockHeaderStore {
    fn add_block_headers<I: IntoIterator<Item = (Epoch, FixedHash, BlockHeader)>>(
        &self,
        headers: I,
    ) -> anyhow::Result<()>;
}

impl<TAddr: NodeAddressable> BaseLayerBlockHeaderStore for GlobalDb<SqliteGlobalDbAdapter<TAddr>> {
    fn add_block_headers<I: IntoIterator<Item = (Epoch, FixedHash, BlockHeader)>>(
        &self,
        headers: I,
    ) -> anyhow::Result<()> {
        let mut tx = self.create_transaction()?;
        let mut header_db = self.block_headers(&mut tx);
        for (epoch, block_hash, header) in headers {
            header_db.insert(BlockHeaderModel {
                epoch,
                height: header.height,
                block_hash,
                kernel_merkle_root: header.kernel_mr,
                validator_node_merkle_root: header.validator_node_mr,
            })?;
        }
        tx.commit()?;
        Ok(())
    }
}
