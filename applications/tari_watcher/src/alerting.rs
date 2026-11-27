// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use anyhow::{Result, bail};
use reqwest::StatusCode;
use serde_json::json;
use url::Url;

pub trait Alerting {
    // Sends an alert message to the service
    async fn alert(&mut self, message: &str) -> Result<()>;

    // Checks that the service is reachable
    async fn ping(&self) -> Result<()>;

    // Statistics on the alerts sent
    // todo: expand granularity and types of stats
    // fn stats(&self) -> Result<u64>;
}

pub struct MatterMostNotifier {
    // Mattermost server URL
    pub server_url: Url,
    // Mattermost channel ID used for alerts
    pub channel_id: String,
    // User token (retrieved after login)
    pub credentials: String,
    // Alerts sent since last reset
    pub alerts_sent: u64,
    // HTTP client
    pub client: reqwest::Client,
}

impl MatterMostNotifier {
    fn endpoint_url(&self, endpoint: &str) -> String {
        format!("{}{}", self.server_url.as_str().trim_end_matches('/'), endpoint)
    }
}

impl Alerting for MatterMostNotifier {
    async fn alert(&mut self, message: &str) -> Result<()> {
        if self.credentials.is_empty() {
            bail!("Credentials field is empty");
        }
        if self.channel_id.is_empty() {
            bail!("Channel ID is empty");
        }

        const POST_ENDPOINT: &str = "/api/v4/posts";
        let url = self.endpoint_url(POST_ENDPOINT);
        let req = json!({
            "channel_id": self.channel_id,
            "message": message,
        });
        let resp = self
            .client
            .post(&url)
            .json(&req)
            .header("Authorization", format!("Bearer {}", self.credentials))
            .send()
            .await?;

        if resp.status() != StatusCode::CREATED {
            bail!("Failed to send alert, got response: {}", resp.status());
        }

        self.alerts_sent += 1;

        Ok(())
    }

    async fn ping(&self) -> Result<()> {
        const PING_ENDPOINT: &str = "/api/v4/users/me";
        if self.credentials.is_empty() {
            bail!("Credentials field is empty");
        }

        let url = self.endpoint_url(PING_ENDPOINT);
        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.credentials))
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            bail!("Failed to ping, got response: {}", resp.status());
        }

        Ok(())
    }

    // fn stats(&self) -> Result<u64> {
    //     Ok(self.alerts_sent)
    // }
}

pub struct TelegramNotifier {
    // Telegram bot token
    pub bot_token: String,
    // Telegram chat ID (either in @channel or number id format)
    pub chat_id: String,
    // Alerts sent since last reset
    pub alerts_sent: u64,
    // HTTP client
    pub client: reqwest::Client,
}

impl Alerting for TelegramNotifier {
    async fn alert(&mut self, message: &str) -> Result<()> {
        if self.bot_token.is_empty() {
            bail!("Bot token is empty");
        }
        if self.chat_id.is_empty() {
            bail!("Chat ID is empty");
        }

        let post_endpoint: &str = &format!("/bot{}/sendMessage", self.bot_token);
        let url = format!("https://api.telegram.org{}", post_endpoint);
        let req = json!({
            "chat_id": self.chat_id,
            "text": message,
        });
        let resp = self.client.post(&url).json(&req).send().await?;

        if resp.status() != StatusCode::OK {
            bail!("Failed to send alert, got response: {}", resp.status());
        }

        self.alerts_sent += 1;

        Ok(())
    }

    async fn ping(&self) -> Result<()> {
        if self.bot_token.is_empty() {
            bail!("Bot token is empty");
        }

        let ping_endpoint: &str = &format!("/bot{}/getMe", self.bot_token);
        let url = format!("https://api.telegram.org{}", ping_endpoint);
        let resp = self.client.get(url.clone()).send().await?;

        if resp.status() != StatusCode::OK {
            bail!("Failed to ping, got response: {}", resp.status());
        }

        Ok(())
    }

    // fn stats(&self) -> Result<u64> {
    //     Ok(self.alerts_sent)
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mattermost_endpoint_url_avoids_double_slash() {
        let notifier = MatterMostNotifier {
            server_url: Url::parse("https://some.corporation.com").unwrap(),
            channel_id: "chan".to_string(),
            credentials: "token".to_string(),
            alerts_sent: 0,
            client: reqwest::Client::new(),
        };

        assert_eq!(
            notifier.endpoint_url("/api/v4/posts"),
            "https://some.corporation.com/api/v4/posts"
        );
        assert_eq!(
            notifier.endpoint_url("/api/v4/users/me"),
            "https://some.corporation.com/api/v4/users/me"
        );
    }
}
