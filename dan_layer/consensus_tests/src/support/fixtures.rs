//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::io::Read;

use serde::de::DeserializeOwned;

pub fn load_json_fixture<T: DeserializeOwned>(name: &str) -> T {
    let path = format!("fixtures/{name}");
    let file = std::fs::File::open(&path).unwrap_or_else(|_| {
        panic!("Could not open fixture file at path: {path}");
    });
    serde_json::from_reader(file).unwrap()
}

pub fn load_binary_fixture(name: &str) -> Vec<u8> {
    let path = format!("fixtures/{name}");
    let mut file = std::fs::File::open(&path).unwrap_or_else(|_| {
        panic!("Could not open fixture file at path: {path}");
    });
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();
    buffer
}
