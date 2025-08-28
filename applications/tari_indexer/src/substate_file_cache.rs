//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fs, path::PathBuf};

use tari_bor::{decode, encode};
use tari_indexer_lib::substate_cache::{SubstateCache, SubstateCacheEntry, SubstateCacheError};

#[derive(Debug, Clone)]
pub struct SubstateFileCache {
    cache_dir_path: String,
}

impl SubstateFileCache {
    pub fn new(path_buf: PathBuf) -> Result<Self, SubstateCacheError> {
        let cache_dir_path = path_buf
            .into_os_string()
            .into_string()
            .map_err(|_| SubstateCacheError("Invalid substate cache path".to_string()))?;

        fs::create_dir_all(&cache_dir_path)
            .map_err(|e| SubstateCacheError(format!("Error creating the cache directory: {}", e)))?;

        Ok(Self { cache_dir_path })
    }
}

impl SubstateCache for SubstateFileCache {
    async fn read(&self, address: String) -> Result<Option<SubstateCacheEntry>, SubstateCacheError> {
        let res = cacache::read(&self.cache_dir_path, address).await;
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

    async fn write(&self, address: String, entry: &SubstateCacheEntry) -> Result<(), SubstateCacheError> {
        let encoded_entry = encode(&entry).map_err(|e| SubstateCacheError(e.to_string()))?;
        cacache::write(&self.cache_dir_path, address, encoded_entry)
            .await
            .map_err(|e| SubstateCacheError(format!("{}", e)))?;
        Ok(())
    }
}
