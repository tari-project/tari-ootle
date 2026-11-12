//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{Epoch, NodeAddressable, optional::Optional};
use tari_ootle_storage::global::{BlockHeaderModel, GlobalDb};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;

pub trait BaseLayerBlockHeaderStore {
    fn add_block_headers<I: IntoIterator<Item = BlockHeaderModel>>(&self, headers: I) -> anyhow::Result<()>;

    /// Returns the lowest-height stored header in the given epoch, or `None` if none are stored. Used
    /// to look up the epoch's boundary block hash.
    fn get_first_block_header_in_epoch(&self, epoch: Epoch) -> anyhow::Result<Option<BlockHeaderModel>>;
}

impl<TAddr: NodeAddressable> BaseLayerBlockHeaderStore for GlobalDb<SqliteGlobalDbAdapter<TAddr>> {
    fn add_block_headers<I: IntoIterator<Item = BlockHeaderModel>>(&self, headers: I) -> anyhow::Result<()> {
        let mut tx = self.create_transaction()?;
        let mut header_db = self.block_headers(&mut tx);
        for header in headers {
            header_db.insert(header)?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get_first_block_header_in_epoch(&self, epoch: Epoch) -> anyhow::Result<Option<BlockHeaderModel>> {
        let mut tx = self.create_transaction()?;
        let header = self
            .block_headers(&mut tx)
            .get_first_block_header_in_epoch(epoch)
            .optional()?;
        Ok(header)
    }
}
