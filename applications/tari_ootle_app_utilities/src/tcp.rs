//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, net::SocketAddr};

use log::warn;
use tokio::net::TcpListener;

const LOG_TARGET: &str = "tari::ootle::app_utilities::tcp";

pub async fn try_bind_with_fallback(mut preferred_address: SocketAddr) -> io::Result<TcpListener> {
    match TcpListener::bind(preferred_address).await {
        Ok(l) => Ok(l),
        Err(e) => {
            warn!(
                target: LOG_TARGET,
                "🕸️ Failed to bind on preferred address ({e}). Trying OS-assigned",
            );
            preferred_address.set_port(0);
            TcpListener::bind(preferred_address).await
        },
    }
}
