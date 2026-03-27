//  Copyright 2023 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    /// Used to create transactions that cost a lot of fees
    pub struct FeeRunnerUpper {
        costly_data: Bytes,
    }

    impl FeeRunnerUpper {
        pub fn new(costly_data: Bytes) -> Self {
            Self { costly_data }
        }

        pub fn set_data(&mut self, data: Bytes) {
            self.costly_data = data;
        }
    }
}
