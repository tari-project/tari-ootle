//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use tari_engine_types::substate::Substate;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_walletd_client::{
    permissions::{Permission, ReadOnly},
    types::{
        SubstatesGetRequest,
        SubstatesGetResponse,
        SubstatesListRequest,
        SubstatesListResponse,
        WalletSubstateInfo,
    },
};

use crate::handlers::{HandlerContext, helpers::not_found};

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SubstatesGetRequest,
) -> Result<SubstatesGetResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.authorize(token, &[Permission::Substates(ReadOnly::Read)])?;

    let record = sdk.substate_api().get_substate(&req.substate_id).optional()?;

    let substate = sdk
        .substate_api()
        .fetch_substate_from_network(&req.substate_id, None)
        .await
        .optional()?;

    if record.is_none() && substate.is_none() {
        return Err(not_found(format!("Substate with ID {} not found", req.substate_id)));
    }

    Ok(SubstatesGetResponse {
        local_record: record.map(|record| WalletSubstateInfo {
            version: record.substate_id.version(),
            substate_id: record.substate_id.into_substate_id(),
            parent_id: record.parent_address,
            module_name: record.module_name,
            template_address: record.template_address,
        }),
        substate_from_remote: substate.map(|s| Substate::new(s.id.version(), s.substate)),
    })
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SubstatesListRequest,
) -> Result<SubstatesListResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.authorize(token, &[Permission::Substates(ReadOnly::Read)])?;
    let substates = sdk.substate_api().list_substates(
        req.filter_by_type,
        req.filter_by_template.as_ref(),
        req.limit,
        req.offset,
    )?;

    let substates = substates
        .into_iter()
        .map(|s| WalletSubstateInfo {
            version: s.substate_id.version(),
            substate_id: s.substate_id.into_substate_id(),
            parent_id: s.parent_address,
            template_address: s.template_address,
            module_name: s.module_name,
        })
        .collect();

    Ok(SubstatesListResponse { substates })
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{
        encoded::Encoded,
        resource_container::ResourceContainer,
        substate::{SubstateId, SubstateValue},
        vault::Vault,
    };
    use tari_ootle_common_types::{SubstateVersion, VersionedSubstateIdRef};
    use tari_template_lib_types::{VaultId, constants::TARI_TOKEN};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use url::Url;

    use super::*;
    use crate::handlers::test_support::TestDaemon;

    #[tokio::test]
    async fn handle_get_fetches_latest_remote_substate_when_local_record_has_older_version() {
        let daemon = TestDaemon::start().await;
        let substate_id = SubstateId::Vault(VaultId::new([0x22; 32].into()));

        daemon
            .context
            .wallet_sdk()
            .substate_api()
            .save_root(VersionedSubstateIdRef::new(&substate_id, SubstateVersion::new(0)), [])
            .unwrap();

        let remote_vault = Vault::new(ResourceContainer::public_fungible(TARI_TOKEN, 500u64));
        let encoded_substate = Encoded::encode(&SubstateValue::Vault(remote_vault.clone())).unwrap();
        let response_json = serde_json::json!({
            "version": 1,
            "state": {
                "up": {
                    "substate": encoded_substate,
                }
            },
            "verified": true,
            "proof": null
        })
        .to_string();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let indexer_url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server_task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let req_text = String::from_utf8_lossy(&buf[..n]);
            let request_line = req_text.lines().next().unwrap_or_default();
            let (status, body) = if request_line.contains("version=") {
                ("404 Not Found", "{}".to_string())
            } else {
                ("200 OK", response_json)
            };
            let http_resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(http_resp.as_bytes()).await.unwrap();
        });

        daemon
            .context
            .wallet_sdk()
            .get_network_interface()
            .set_endpoints(vec![indexer_url])
            .unwrap();

        let resp = handle_get(&daemon.context, Some(&daemon.session), SubstatesGetRequest {
            substate_id: substate_id.clone(),
        })
        .await
        .unwrap();

        server_task.await.unwrap();

        let local = resp.local_record.expect("local record present");
        assert_eq!(local.substate_id, substate_id);
        assert_eq!(local.version, SubstateVersion::new(0));

        let remote = resp.substate_from_remote.expect("remote substate fetched");
        assert_eq!(remote.version(), SubstateVersion::new(1));
        assert_eq!(
            remote.substate_value().as_vault().unwrap().balance(),
            remote_vault.balance()
        );
    }
}
