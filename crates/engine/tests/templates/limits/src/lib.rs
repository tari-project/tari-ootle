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

        pub fn request_random_bytes(len: u32) -> Vec<u8> {
            rand::random_bytes(len)
        }
    }
}
