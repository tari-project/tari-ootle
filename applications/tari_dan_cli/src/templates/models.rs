// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};
use termimad::{crossterm::style::Color, MadSkin};

#[derive(Clone, Debug)]
pub struct Template {
    path: PathBuf,
    id: String,
    name: String,
    description: String,
    extra: HashMap<String, String>,
}

impl Display for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut skin = MadSkin::default();
        skin.bold.set_fg(Color::Magenta);
        let formatted_name = skin.inline(format!("**{}**", self.name).as_str()).to_string();
        let formatted_description = skin.inline(self.description.as_str()).to_string();
        write!(f, "{} - {}", formatted_name, formatted_description)
    }
}

impl Template {
    pub fn new(path: PathBuf, id: String, name: String, description: String, extra: HashMap<String, String>) -> Self {
        Self {
            path,
            id,
            name,
            description,
            extra,
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn extra(&self) -> &HashMap<String, String> {
        &self.extra
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateFile {
    pub name: String,
    pub description: String,
    pub extra: Option<HashMap<String, String>>,
}
