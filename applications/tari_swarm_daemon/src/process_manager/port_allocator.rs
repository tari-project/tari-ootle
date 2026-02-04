//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, atomic, atomic::AtomicU16},
};

use tokio::net::TcpListener;

use crate::{
    config::{InstanceConfig, InstanceType},
    process_manager::InstanceId,
};

pub struct PortAllocator {
    instances: HashMap<InstanceId, AllocatedPorts>,
    start_port_overrides: HashMap<InstanceType, Arc<AtomicU16>>,
    current_port: Arc<AtomicU16>,
}

impl PortAllocator {
    pub fn new(start_port: u16, instance_config: &[InstanceConfig]) -> Self {
        Self {
            instances: HashMap::new(),
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
        }
    }

    pub fn register(&mut self, instance_id: InstanceId, ports: AllocatedPorts) {
        self.instances.insert(instance_id, ports);
    }

    pub fn unregister(&mut self, instance_id: InstanceId) {
        self.instances.remove(&instance_id);
    }
}

#[derive(Debug, Clone)]
pub struct AllocatedPorts {
    current_port: Arc<AtomicU16>,
    ports: HashMap<&'static str, u16>,
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

    pub async fn get_or_next_port(&mut self, name: &'static str) -> u16 {
        if let Some(port) = self.ports.get(name) {
            return *port;
        }
        loop {
            let port = self.next_port();
            if check_local_port(port).await {
                log::debug!("Port {port} is free for {name}");
                self.ports.insert(name, port);
                return port;
            }
        }
    }
}

// pub struct InstancePortAllocator<'a> {
//     ports: &'a mut HashMap<&'static str, u16>,
//     current_port: &'a mut u16,
// }
//
//

async fn check_local_port(port: u16) -> bool {
    log::debug!("Checking port {}", port);
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .is_ok()
}
