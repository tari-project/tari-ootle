//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::{Path, PathBuf};

pub struct TemplateSpec {
    pub path: PathBuf,
    pub features: Vec<&'static str>,
}

impl From<&str> for TemplateSpec {
    fn from(path: &str) -> Self {
        Self {
            path: path.into(),
            features: vec![],
        }
    }
}

impl From<&&str> for TemplateSpec {
    fn from(path: &&str) -> Self {
        Self {
            path: path.into(),
            features: vec![],
        }
    }
}
impl From<&Path> for TemplateSpec {
    fn from(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            features: vec![],
        }
    }
}

impl From<PathBuf> for TemplateSpec {
    fn from(path: PathBuf) -> Self {
        Self { path, features: vec![] }
    }
}

impl From<(&str, &[&'static str])> for TemplateSpec {
    fn from((path, features): (&str, &[&'static str])) -> Self {
        Self {
            path: path.into(),
            features: features.to_vec(),
        }
    }
}
