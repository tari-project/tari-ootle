//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError, atomic, atomic::AtomicU16},
};

use tokio::net::TcpListener;

use crate::{
    config::{InstanceConfig, InstanceType},
    process_manager::InstanceId,
};

pub struct PortAllocator {
    instances: Arc<Mutex<HashMap<InstanceId, AllocatedPorts>>>,
    start_port_overrides: HashMap<InstanceType, Arc<AtomicU16>>,
    current_port: Arc<AtomicU16>,
}

impl PortAllocator {
    pub fn new(start_port: u16, instance_config: &[InstanceConfig]) -> Self {
        Self {
            instances: Arc::new(Mutex::new(HashMap::new())),
            start_port_overrides: instance_config
                .iter()
                .filter_map(|c| {
                    c.start_port_override
                        .map(|p| (c.instance_type, Arc::new(AtomicU16::new(p))))
                })
                .collect(),
            current_port: Arc::new(AtomicU16::new(start_port)),
        }
    }

    pub fn create(&mut self, instance_type: InstanceType) -> AllocatedPorts {
        AllocatedPorts {
            ports: HashMap::new(),
            current_port: self
                .start_port_overrides
                .get(&instance_type)
                .cloned()
                .unwrap_or_else(|| self.current_port.clone()),
            registered_instances: self.instances.clone(),
        }
    }

    pub fn register(&mut self, instance_id: InstanceId, ports: AllocatedPorts) {
        self.instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(instance_id, ports);
    }

    pub fn unregister(&mut self, instance_id: InstanceId) {
        self.instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&instance_id);
    }
}

#[derive(Debug, Clone)]
pub struct AllocatedPorts {
    current_port: Arc<AtomicU16>,
    ports: HashMap<&'static str, u16>,
    registered_instances: Arc<Mutex<HashMap<InstanceId, AllocatedPorts>>>,
}

impl AllocatedPorts {
    pub fn get(&self, name: &'static str) -> Option<u16> {
        self.ports.get(name).copied()
    }

    pub fn expect(&self, name: &'static str) -> u16 {
        self.ports[name]
    }

    pub fn into_ports(self) -> HashMap<&'static str, u16> {
        self.ports
    }

    fn next_port(&self) -> u16 {
        self.current_port.fetch_add(1, atomic::Ordering::SeqCst)
    }

    fn is_port_in_use_by_swarm(&self, port: u16) -> bool {
        if self.ports.values().any(|&allocated| allocated == port) {
            return true;
        }
        self.registered_instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|instance_ports| instance_ports.ports.values().any(|&allocated| allocated == port))
    }

    /// Prefer get_or_next_port for most use cases, but this can be used to set a specific port for a name.
    pub fn set_port(&mut self, name: &'static str, port: u16) {
        self.ports.insert(name, port);
    }

    pub async fn get_or_next_port(&mut self, name: &'static str) -> u16 {
        if let Some(port) = self.ports.get(name) {
            return *port;
        }
        loop {
            let port = self.next_port();
            if port == 0 || self.is_port_in_use_by_swarm(port) {
                continue;
            }
            if check_local_port(port).await {
                log::debug!("Port {port} is free for {name}");
                self.ports.insert(name, port);
                return port;
            }
        }
    }
}

async fn check_local_port(port: u16) -> bool {
    log::debug!("Checking port {}", port);
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn port_allocator_skips_ports_registered_to_other_instances_until_unregistered() {
        let override_cfg = InstanceConfig {
            start_port_override: Some(49100),
            ..InstanceConfig::new(InstanceType::TariValidatorNode)
        };
        let mut allocator = PortAllocator::new(49100, &[override_cfg]);

        let mut base_ports = allocator.create(InstanceType::MinoTariNode);
        let first_port = base_ports.get_or_next_port("grpc").await;
        allocator.register(1, base_ports);

        let mut vn_ports = allocator.create(InstanceType::TariValidatorNode);
        let second_port = vn_ports.get_or_next_port("jrpc").await;
        assert_ne!(
            first_port, second_port,
            "override counter starting at the same port must skip already-registered port"
        );

        allocator.unregister(1);
        let mut replacement = PortAllocator::new(first_port, &[]);
        let mut reused = replacement.create(InstanceType::MinoTariNode);
        assert_eq!(reused.get_or_next_port("grpc").await, first_port);
    }

    #[tokio::test]
    async fn port_allocator_never_allocates_port_zero_on_wraparound() {
        let mut allocator = PortAllocator::new(0, &[]);
        let mut ports = allocator.create(InstanceType::MinoTariNode);
        let port = ports.get_or_next_port("p2p").await;
        assert_ne!(port, 0);
    }
}
