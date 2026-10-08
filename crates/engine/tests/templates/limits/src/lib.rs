//  Copyright 2025 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    pub struct PushItToTheLimit {
        data: Bytes,
    }

    impl PushItToTheLimit {
        pub fn new(data: Bytes) -> Self {
            Self { data }
        }

        pub fn set_data(&mut self, data: Bytes) {
            self.data = data;
        }


        /// Returns `len` zeros, each encoded as a one-byte CBOR item.
        pub fn return_items(len: u32) -> Vec<u32> {
            vec![0; len as usize]
        }

        /// Calls `return_items` on `template` and returns how many items came back.
        pub fn return_items_nested(template: TemplateAddress, len: u32) -> u32 {
            let items: Vec<u32> = TemplateManager::get(template).call("return_items", args![len]);
            items.len() as u32
        }

        pub fn request_random_bytes(len: u32) -> Vec<u8> {
            rand::random_bytes(len)
        }

        pub fn emit_log_of_size(len: u32) {
            debug!("{}", "a".repeat(len as usize));
        }

        /// Emits an event whose payload holds a single `len`-byte value under the key `data`.
        pub fn emit_event_of_size(len: u32) {
            let mut payload = Metadata::new();
            payload.insert("data", &"a".repeat(len as usize));
            emit_event("big", payload);
        }
    }
}
