//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fs, path::PathBuf};

use tari_bor::{decode, encode};
use tari_engine_types::substate::SubstateId;
use tari_indexer_lib::substate_cache::{SubstateCache, SubstateCacheEntry, SubstateCacheEntryRef, SubstateCacheError};

#[derive(Debug, Clone)]
pub struct SubstateFileCache {
    cache_dir_path: PathBuf,
}

impl SubstateFileCache {
    pub fn new(cache_dir_path: PathBuf) -> Result<Self, SubstateCacheError> {
        fs::create_dir_all(&cache_dir_path)
            .map_err(|e| SubstateCacheError(format!("Error creating the cache directory: {}", e)))?;

        Ok(Self { cache_dir_path })
    }
}

impl SubstateCache for SubstateFileCache {
    async fn read(&self, id: &SubstateId) -> Result<Option<SubstateCacheEntry>, SubstateCacheError> {
        let res = cacache::read(&self.cache_dir_path, id.to_address_string()).await;
        match res {
            Ok(value) => {
                // cache hit
                let entry = decode::<SubstateCacheEntry>(&value).map_err(|e| SubstateCacheError(e.to_string()))?;
                Ok(Some(entry))
            },
            Err(e) => {
                // cache miss
                if let cacache::Error::EntryNotFound(_, _) = e {
                    Ok(None)
                // cache error
                } else {
                    Err(SubstateCacheError(format!("{}", e)))
                }
            },
        }
    }

    async fn write(&self, id: &SubstateId, entry: SubstateCacheEntryRef<'_>) -> Result<(), SubstateCacheError> {
        let encoded_entry = encode(&entry).map_err(|e| SubstateCacheError(e.to_string()))?;
        cacache::write(&self.cache_dir_path, id.to_address_string(), encoded_entry)
            .await
            .map_err(|e| SubstateCacheError(format!("{}", e)))?;
        Ok(())
    }
}
