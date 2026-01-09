//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    pub struct NoConcurrency {}

    impl NoConcurrency {
        pub fn try_to_spawn_a_thread_static() {
            std::thread::spawn(|| {
                // Try an engine call for fun, hopefully we cant get here
                CallerContext::transaction_signer_public_key();
                std::thread::sleep(std::time::Duration::from_millis(100));
                unreachable!("Should not be able to spawn threads");
            })
            .join()
            .unwrap();
        }
    }
}
