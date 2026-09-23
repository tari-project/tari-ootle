//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_wallet_sdk::apis::config::ConfigKey;
use tari_ootle_walletd_client::{
    permissions::{Crud, Permission},
    types::{NetworkInfo, SettingsGetResponse, SettingsSetRequest, SettingsSetResponse},
};
use url::Url;

use crate::handlers::{HandlerContext, helpers::invalid_params};

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    _value: serde_json::Value,
) -> Result<SettingsGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.authorize(token, &[Permission::Settings(Crud::Read)])?;
    let indexer_url = sdk
        .config_api()
        .get(ConfigKey::IndexerUrl)
        .optional()?
        .unwrap_or_else(|| sdk.get_network_interface().get_endpoint());
    let network = sdk.config_api().get_network()?;
    let advanced_ui_features = sdk
        .config_api()
        .get(ConfigKey::AdvancedUiFeatures)
        .optional()?
        .unwrap_or_default();
    let claimed_accounts = sdk
        .config_api()
        .get(ConfigKey::ClaimedAccounts)
        .optional()?
        .unwrap_or_default();

    // Deliberately not fatal: the indexer being down must not make settings unreadable, since this
    // is where the indexer URL is corrected.
    let current_epoch = context.current_epoch().await.ok();

    Ok(SettingsGetResponse {
        indexer_url,
        network: NetworkInfo {
            name: network.to_string(),
            byte: network.as_byte(),
        },
        advanced_ui_features,
        claimed_accounts,
        current_epoch,
        default_transaction_validity_epochs: context.config().default_transaction_validity_epochs,
    })
}

/// The permissions `settings.set` requires of its caller, decided by which fields the request
/// carries.
///
/// `indexer_url` chooses which server the wallet believes is the chain: what it reports as its own
/// balances, whether a transaction it submits is ever broadcast, and who learns of every transaction
/// it does submit. That is an administrative decision about the wallet's trust, so it takes `Admin`.
/// The remaining fields are UI preferences and take `Settings(Update)`, which is what a client
/// holding only a preference scope is for.
fn required_permissions(req: &SettingsSetRequest) -> Vec<Permission> {
    if req.indexer_url.is_some() {
        // `Admin` satisfies `Settings(Update)`, so it alone covers a request that also carries
        // preference fields.
        vec![Permission::Admin]
    } else {
        vec![Permission::Settings(Crud::Update)]
    }
}

/// Refuses an indexer URL that cannot name a server the wallet can talk to.
///
/// The indexer is reached over HTTP, so only `http` and `https` can address one; any other scheme
/// would be stored and installed as the live endpoint and then fail on every query, far from the
/// call that set it. Embedded credentials are refused because the endpoint is persisted in the
/// config store and appears in `settings.get`, so a password in it leaks by being stored at all.
///
/// No refusal formats the `Url` itself. `Url`'s `Display` is its full serialization including
/// userinfo, and a rejection message is rendered into the JSON-RPC error and warned to the log, so a
/// message carrying the URL would write the very password this refuses to store.
///
/// The host is deliberately unconstrained. A wallet's indexer normally runs on loopback or on the
/// local network, so refusing private ranges would reject the ordinary deployment; `Admin` is what
/// separates a caller allowed to choose it from one that is not.
fn validate_indexer_url(url: &Url) -> Result<(), anyhow::Error> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid_params(
            "indexer_url",
            Some("The indexer URL is persisted in the wallet's settings and must not embed credentials"),
        ));
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid_params(
            "indexer_url",
            Some(format!(
                "The indexer is reached over HTTP, but the URL has scheme '{}'",
                url.scheme()
            )),
        ));
    }
    Ok(())
}

pub async fn handle_set(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SettingsSetRequest,
) -> Result<SettingsSetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk();
    context.authorize(token, &required_permissions(&req))?;
    if let Some(indexer_url) = req.indexer_url {
        validate_indexer_url(&indexer_url)?;
        sdk.config_api().set(ConfigKey::IndexerUrl, &indexer_url)?;
        sdk.get_network_interface().set_endpoint(indexer_url);
        // The cached epoch describes the indexer we just stopped using.
        context.invalidate_epoch_cache();
    }
    if let Some(advanced_ui_features) = &req.advanced_ui_features {
        sdk.config_api()
            .set(ConfigKey::AdvancedUiFeatures, advanced_ui_features)?;
    }
    if let Some(claimed_accounts) = &req.claimed_accounts {
        sdk.config_api().set(ConfigKey::ClaimedAccounts, claimed_accounts)?;
    }
    Ok(SettingsSetResponse {})
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tari_ootle_walletd_client::permissions::Permissions;

    use super::*;

    fn request(indexer_url: Option<&str>) -> SettingsSetRequest {
        SettingsSetRequest {
            indexer_url: indexer_url.map(|url| Url::parse(url).unwrap()),
            advanced_ui_features: None,
            claimed_accounts: None,
        }
    }

    fn granted(permissions: &str) -> Permissions {
        Permissions::from_str(permissions).unwrap()
    }

    #[test]
    fn repointing_the_indexer_is_refused_to_a_preference_scope() {
        let required = required_permissions(&request(Some("http://127.0.0.1:18300")));
        assert!(granted("settings:update").check(&required).is_err());
        assert!(granted("admin").check(&required).is_ok());
    }

    /// The preference fields must stay reachable without `Admin`, or the web UI's own settings page
    /// would need an administrative token to toggle a checkbox.
    #[test]
    fn changing_a_preference_does_not_need_admin() {
        let required = required_permissions(&request(None));
        assert!(granted("settings:update").check(&required).is_ok());
    }

    #[test]
    fn a_non_http_indexer_url_is_refused() {
        validate_indexer_url(&Url::parse("file:///etc/passwd").unwrap()).unwrap_err();
        validate_indexer_url(&Url::parse("data:text/plain,hello").unwrap()).unwrap_err();
    }

    #[test]
    fn an_indexer_url_embedding_credentials_is_refused() {
        validate_indexer_url(&Url::parse("http://user:pass@indexer.example/").unwrap()).unwrap_err();
        validate_indexer_url(&Url::parse("http://user@indexer.example/").unwrap()).unwrap_err();
    }

    /// A refusal is rendered into the JSON-RPC error and warned to the log, so no branch of it may
    /// echo the URL -- `Url`'s `Display` carries userinfo.
    #[test]
    fn no_refusal_echoes_the_credentials() {
        for url in [
            "ftp://user:swordfish@indexer.example/",
            "http://user:swordfish@indexer.example/",
        ] {
            let err = validate_indexer_url(&Url::parse(url).unwrap()).unwrap_err().to_string();
            assert!(!err.contains("swordfish"), "{err}");
        }
    }

    /// A loopback indexer is the default deployment, so no host restriction may reject it.
    #[test]
    fn a_loopback_indexer_url_is_accepted() {
        validate_indexer_url(&Url::parse("http://127.0.0.1:18300").unwrap()).unwrap();
        validate_indexer_url(&Url::parse("https://indexer.example/json_rpc").unwrap()).unwrap();
    }
}
