//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! This is a hack to get around a circular dependency between the epoch manager and template manager services
//! In the current implementation (at the time of writing), the circular dependency cannot result in a deadlock.
//! However, later changes could cause this to change.

use tari_common_types::types::FixedHash;
use tari_dan_common_types::Epoch;
use tari_epoch_manager::traits::TemplateDownloader;
use tari_template_lib::{prelude::RistrettoPublicKeyBytes, types::TemplateAddress};
use tari_template_manager::interface::{AddTemplateRequest, TemplateExecutable, TemplateManagerError};
use tokio::sync::mpsc;

pub struct TemplateDownloadQueue {
    tx: mpsc::UnboundedSender<AddTemplateRequest>,
}

impl TemplateDownloadQueue {
    pub fn create() -> (Self, mpsc::UnboundedReceiver<AddTemplateRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

impl TemplateDownloader for TemplateDownloadQueue {
    type Error = TemplateManagerError;

    async fn enqueue_download(
        &mut self,
        epoch: Epoch,
        name: String,
        address: TemplateAddress,
        author_public_key: RistrettoPublicKeyBytes,
        url: url::Url,
        binary_hash: FixedHash,
    ) -> Result<(), Self::Error> {
        if self
            .tx
            .send(AddTemplateRequest {
                author_public_key,
                template_address: address,
                template: TemplateExecutable::DownloadableWasm(url, binary_hash),
                template_name: Some(name),
                epoch,
            })
            .is_err()
        {
            return Err(TemplateManagerError::ChannelClosed);
        }
        Ok(())
    }
}
