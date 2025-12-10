//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use serde_json as json;
use tari_state_store_rocksdb::traits::Cf;

use crate::webserver::error::WebError;

#[derive(Debug, Clone, Deserialize)]
pub struct TableRequest {
    pub query: Option<String>,
    pub page: Option<usize>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Column {
    pub field: String,
    pub label: String,
}

impl Column {
    pub fn new<T: Into<String>, U: Into<String>>(field: T, label: U) -> Self {
        Self {
            field: field.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TableResponse {
    columns: Vec<Column>,
    rows: Vec<json::Value>,
    total_entries: Option<usize>,
    total_bytes: usize,
    largest_row_size: usize,
    smallest_row_size: usize,
}

impl TableResponse {
    pub fn empty() -> Self {
        Self {
            columns: vec![],
            rows: vec![],
            total_entries: None,
            total_bytes: 0,
            largest_row_size: 0,
            smallest_row_size: 0,
        }
    }

    pub fn new<I: IntoIterator<Item = Column>>(columns: I) -> Self {
        Self {
            columns: columns.into_iter().collect(),
            rows: vec![],
            total_entries: None,
            total_bytes: 0,
            largest_row_size: 0,
            smallest_row_size: 0,
        }
    }

    pub fn add_row(&mut self, row: json::Value) -> &mut Self {
        self.rows.push(row);
        self
    }

    pub fn with_columns<I: IntoIterator<Item = Column>>(&mut self, columns: I) -> &mut Self {
        self.columns = columns.into_iter().collect();
        self
    }

    pub fn set_total_entries(&mut self, count: usize) -> &mut Self {
        self.total_entries = Some(count);
        self
    }

    pub fn set_total_bytes(&mut self, count: usize) -> &mut Self {
        self.total_bytes = count;
        self
    }

    pub fn set_largest_row_size(&mut self, size: usize) -> &mut Self {
        self.largest_row_size = size;
        self
    }

    pub fn set_smallest_row_size(&mut self, size: usize) -> &mut Self {
        self.smallest_row_size = size;
        self
    }
}

#[macro_export]
macro_rules! row {
    ($($s:expr),*$(,)?) => {
        vec![$(::serde_json::to_value(&$s).unwrap()),*]
    }
}

pub fn decode_hex_prefix<CF: Cf>(prefix_hex: &str) -> Result<Vec<u8>, WebError> {
    let mut prefix = vec![0u8; prefix_hex.len().div_ceil(2) + 1];
    if prefix_hex.len().is_multiple_of(2) {
        hex::decode_to_slice(prefix_hex, &mut prefix[1..])
            .map_err(|e| WebError::bad_request(format!("Failed to decode hex prefix: {}. Error: {}", prefix_hex, e)))?;
    } else {
        let mut p = prefix_hex.to_string();
        p.push('0');
        hex::decode_to_slice(p, &mut prefix[1..])
            .map_err(|e| WebError::bad_request(format!("Failed to decode hex prefix: {}. Error: {}", prefix_hex, e)))?;
    }

    if let Some(p) = CF::key_prefix() {
        prefix[0] = p;
    } else {
        prefix.remove(0);
    }
    Ok(prefix)
}
