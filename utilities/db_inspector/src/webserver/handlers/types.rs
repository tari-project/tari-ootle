//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use serde_json as json;

#[derive(Debug, Clone, Deserialize)]
pub struct TableRequest {
    pub limit: Option<usize>,
    #[serde(default)]
    pub asc: bool,
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
}

impl TableResponse {
    pub fn empty() -> Self {
        Self {
            columns: vec![],
            rows: vec![],
            total_entries: None,
        }
    }

    pub fn new<I: IntoIterator<Item = Column>>(columns: I) -> Self {
        Self {
            columns: columns.into_iter().collect(),
            rows: vec![],
            total_entries: None,
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
}

#[macro_export]
macro_rules! row {
    ($($s:expr),*$(,)?) => {
        vec![$(::serde_json::to_value(&$s).unwrap()),*]
    }
}
