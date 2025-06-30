//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

pub trait Versioned: Sized {
    type Latest;
    fn upgrade_single_step(self) -> (Self, bool);

    fn full_upgrade(self) -> Self {
        let mut current = self;

        loop {
            let (next, step_upgraded) = current.upgrade_single_step();
            if !step_upgraded {
                return next;
            }
            current = next;
        }
    }

    fn into_latest(self) -> Self::Latest;

    fn full_upgrade_and_into_latest(self) -> Self::Latest {
        self.full_upgrade().into_latest()
    }
}
