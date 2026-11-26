//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, Instant};

use log::warn;
use serde::{Deserialize, Serialize};
use tari_shutdown::Shutdown;
use tokio::select;

use crate::webserver::context::HandlerContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MineRequest {
    num_blocks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MineResponse {}

pub async fn mine(context: &HandlerContext, req: MineRequest) -> Result<MineResponse, anyhow::Error> {
    context.process_manager().mine_blocks(req.num_blocks).await?;
    Ok(MineResponse {})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartMiningRequest {
    interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartMiningResponse {}

pub async fn start_mining(
    context: &HandlerContext,
    req: StartMiningRequest,
) -> Result<StartMiningResponse, anyhow::Error> {
    let interval_duration = validate_mining_interval(req.interval_seconds)?;
    let shutdown = Shutdown::new();
    context.start_mining(shutdown.clone()).await?;

    let process_manager = context.process_manager().clone();
    tokio::spawn(async move {
        let shutdown_signal = shutdown.to_signal();
        let interval = tokio::time::interval(interval_duration);
        tokio::pin!(shutdown_signal);
        tokio::pin!(interval);
        loop {
            select! {
                _ = &mut shutdown_signal => {
                    break;
                }
                _ = interval.tick() => {
                    if let Err(error) = process_manager.mine_blocks(1).await {
                        warn!("Failed to mine a block: {error:?}");
                    }
                }
            }
        }
    });
    Ok(StartMiningResponse {})
}

/// Validates that the auto-mining period is non-zero before calling `tokio::time::interval`,
/// which panics on `Duration::ZERO`, and small enough not to overflow `Instant` when
/// `tokio::time::Interval` advances its deadline (`timeout + period`).
fn validate_mining_interval(interval_seconds: u64) -> Result<Duration, anyhow::Error> {
    if interval_seconds == 0 {
        anyhow::bail!("interval_seconds must be greater than 0");
    }
    let period = Duration::from_secs(interval_seconds);
    if period
        .checked_mul(2)
        .and_then(|d| Instant::now().checked_add(d))
        .is_none()
    {
        anyhow::bail!("interval_seconds is too large");
    }
    Ok(period)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsMiningRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsMiningResponse {
    result: bool,
}

pub async fn is_mining(context: &HandlerContext, _req: IsMiningRequest) -> Result<IsMiningResponse, anyhow::Error> {
    Ok(IsMiningResponse {
        result: context.is_mining().await,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopMiningRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopMiningResponse {
    result: bool,
}

pub async fn stop_mining(
    context: &HandlerContext,
    _req: StopMiningRequest,
) -> Result<StopMiningResponse, anyhow::Error> {
    context.stop_mining().await;
    Ok(StopMiningResponse { result: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_mining_interval_rejects_zero() {
        assert!(validate_mining_interval(0).is_err());
    }

    #[test]
    fn validate_mining_interval_rejects_overflow() {
        assert!(validate_mining_interval(u64::MAX).is_err());
    }

    #[test]
    fn validate_mining_interval_accepts_nonzero() {
        assert_eq!(validate_mining_interval(5).unwrap(), Duration::from_secs(5));
    }
}
