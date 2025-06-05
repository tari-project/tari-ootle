//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::registry::{Metric, Registry};

pub trait CollectorRegister {
    fn register_at<N: Into<String>, H: Into<String>>(self, name: N, help: H, registry: &mut Registry) -> Self;
}

impl<C: Metric + Clone + 'static> CollectorRegister for C {
    fn register_at<N: Into<String>, H: Into<String>>(self, name: N, help: H, registry: &mut Registry) -> Self {
        registry.register(name, help, self.clone());
        self
    }
}
