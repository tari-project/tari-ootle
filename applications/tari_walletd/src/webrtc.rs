//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum_jrpc::{JsonRpcAnswer, JsonRpcRequest, JsonRpcResponse};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json as json;
use serde_json::json;
use tari_ootle_walletd_client::types::EncodedJwtString;
use tari_shutdown::ShutdownSignal;
use tokio::sync::Notify;
use webrtc::{
    api::APIBuilder,
    data_channel::{RTCDataChannel, data_channel_message::DataChannelMessage},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    peer_connection::{
        RTCPeerConnection,
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::webrtc";

#[derive(Deserialize, Debug)]
struct Request {
    id: u64,
    method: String,
    #[serde(default)]
    params: json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
struct Response {
    id: u64,
    payload: json::Value,
}

fn signaling_server_url(address: SocketAddr) -> String {
    format!("http://{address}")
}

fn wallet_jrpc_url(address: SocketAddr) -> String {
    let ip = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}/json_rpc", SocketAddr::new(ip, address.port()))
}

/// Methods that mint credentials, mutate daemon sessions/passkeys/API keys, or open nested WebRTC
/// sessions must never be reachable over a delegated WebRTC data channel.
fn is_disallowed_webrtc_method(method: &str) -> bool {
    matches!(
        method.split_once('.').map(|(namespace, _)| namespace),
        Some("auth" | "webauthn" | "webrtc")
    )
}

const fn is_terminal_peer_connection_state(state: RTCPeerConnectionState) -> bool {
    matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed)
}

async fn make_request<T: Serialize>(
    url: &str,
    token: Option<&str>,
    method: String,
    params: T,
) -> anyhow::Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let body = JsonRpcRequest {
        id: axum_jrpc::Id::Num(0),
        method,
        params: serde_json::to_value(params)?,
    };
    let mut builder = client.post(url).header(CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let resp = builder.json(&body).send().await?.json::<JsonRpcResponse>().await?;
    match resp.result {
        JsonRpcAnswer::Result(result) => Ok(result),
        JsonRpcAnswer::Error(error) => Err(anyhow::Error::msg(error.to_string())),
    }
}

fn get_rtc_configuration() -> RTCConfiguration {
    RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    }
}

pub async fn on_ice_candidate(
    ice_candidate: Option<RTCIceCandidate>,
    signaling_server_token_clone: String,
    signaling_server_address: SocketAddr,
) {
    if let Some(ice_candidate) = ice_candidate {
        tokio::task::spawn(async move {
            match ice_candidate.to_json() {
                Ok(ice_candidate) => {
                    if let Err(err) = make_request(
                        &signaling_server_url(signaling_server_address),
                        Some(signaling_server_token_clone.as_str()),
                        "add.answer_ice_candidate".to_string(),
                        ice_candidate,
                    )
                    .await
                    {
                        log::error!(target: LOG_TARGET, "Error sending ice candidate: {}", err);
                    }
                },
                Err(e) => {
                    log::error!(target: LOG_TARGET, "Error sending ice candidate: {}", e);
                },
            };
        });
    }
}

async fn handle_webrtc_request(
    raw_msg: &[u8],
    permissions_token: &EncodedJwtString,
    address: SocketAddr,
) -> anyhow::Result<Response> {
    let request: Request = serde_json::from_slice(raw_msg)?;

    let payload = if request.method == "get.token" {
        json::to_value(permissions_token)?
    } else if is_disallowed_webrtc_method(&request.method) {
        json!({
            "error": format!("Method '{}' is not permitted over WebRTC", request.method)
        })
    } else {
        make_request(
            &wallet_jrpc_url(address),
            Some(permissions_token.as_str()),
            request.method,
            request.params,
        )
        .await
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
    };

    Ok(Response {
        id: request.id,
        payload,
    })
}

pub async fn on_message(
    msg: DataChannelMessage,
    d_on_message: Arc<RTCDataChannel>,
    permissions_token: &EncodedJwtString,
    address: SocketAddr,
) -> anyhow::Result<()> {
    let response = handle_webrtc_request(msg.data.as_ref(), permissions_token, address).await?;
    let text = serde_json::to_string(&response)?;
    d_on_message.send_text(text).await?;
    Ok(())
}

pub async fn on_data_channel(d: Arc<RTCDataChannel>, permissions_token: EncodedJwtString, address: SocketAddr) {
    let d_on_message = d.clone();
    d.on_message(Box::new(move |msg: DataChannelMessage| {
        let d_on_message = d_on_message.clone();
        let permissions_token = permissions_token.clone();
        Box::pin(async move {
            if let Err(err) = on_message(msg, d_on_message.clone(), &permissions_token, address).await {
                log::error!(target: LOG_TARGET, "Error handling message: {}", err);
            }
        })
    }))
}

async fn negotiate_webrtc_session(
    pc: &RTCPeerConnection,
    signaling_server_token: &str,
    signaling_server_address: SocketAddr,
) -> anyhow::Result<()> {
    let signaling_url = signaling_server_url(signaling_server_address);
    let offer = make_request(
        &signaling_url,
        Some(signaling_server_token),
        "get.offer".to_string(),
        json!({}),
    )
    .await?;

    let desc = RTCSessionDescription::offer(
        offer
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("RTC Offer error"))?
            .to_string(),
    )?;
    pc.set_remote_description(desc).await?;

    let ices = make_request(
        &signaling_url,
        Some(signaling_server_token),
        "get.offer_ice_candidates".to_string(),
        json!({}),
    )
    .await?;

    let ices: Vec<RTCIceCandidateInit> = serde_json::from_value(ices)?;
    for ice_candidate in ices {
        log::debug!(target: LOG_TARGET, "ice_candidate {:?}", ice_candidate);
        pc.add_ice_candidate(ice_candidate).await?;
    }
    let answer = pc.create_answer(None).await?;
    pc.set_local_description(answer.clone()).await?;

    make_request(
        &signaling_url,
        Some(signaling_server_token),
        "add.answer".to_string(),
        &answer.sdp,
    )
    .await?;

    Ok(())
}

pub async fn webrtc_start_session(
    signaling_server_token: String,
    permissions_token: EncodedJwtString,
    address: SocketAddr,
    signaling_server_address: SocketAddr,
    shutdown_signal: ShutdownSignal,
) -> anyhow::Result<()> {
    webrtc_start_session_with_config(
        signaling_server_token,
        permissions_token,
        address,
        signaling_server_address,
        shutdown_signal,
        get_rtc_configuration(),
    )
    .await
}

async fn webrtc_start_session_with_config(
    signaling_server_token: String,
    permissions_token: EncodedJwtString,
    address: SocketAddr,
    signaling_server_address: SocketAddr,
    shutdown_signal: ShutdownSignal,
    rtc_configuration: RTCConfiguration,
) -> anyhow::Result<()> {
    let api = APIBuilder::new().build();
    let pc = api.new_peer_connection(rtc_configuration).await?;

    let peer_closed = Arc::new(Notify::new());
    let peer_closed_on_state = peer_closed.clone();
    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        log::debug!(target: LOG_TARGET, "WebRTC peer connection state changed: {state}");
        if is_terminal_peer_connection_state(state) {
            peer_closed_on_state.notify_one();
        }
        Box::pin(async {})
    }));

    let open_channels = Arc::new(AtomicUsize::new(0));
    let peer_closed_on_dc = peer_closed.clone();
    pc.on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
        open_channels.fetch_add(1, Ordering::AcqRel);
        let open_channels_on_close = open_channels.clone();
        let peer_closed_on_close = peer_closed_on_dc.clone();
        d.on_close(Box::new(move || {
            if open_channels_on_close.fetch_sub(1, Ordering::AcqRel) == 1 {
                peer_closed_on_close.notify_one();
            }
            Box::pin(async {})
        }));
        Box::pin(on_data_channel(d, permissions_token.clone(), address))
    }));

    let signaling_server_token_clone = signaling_server_token.clone();
    pc.on_ice_candidate(Box::new(move |ice_candidate: Option<RTCIceCandidate>| {
        Box::pin(on_ice_candidate(
            ice_candidate,
            signaling_server_token_clone.clone(),
            signaling_server_address,
        ))
    }));

    let session_result = async {
        negotiate_webrtc_session(&pc, signaling_server_token.as_str(), signaling_server_address).await?;
        tokio::select! {
            _ = shutdown_signal => {},
            _ = peer_closed.notified() => {
                log::info!(target: LOG_TARGET, "WebRTC peer connection closed or failed; closing session");
            },
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    let close_result = pc.close().await;
    session_result?;
    close_result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Duration};

    use axum::{Json, Router, extract::State, routing::post};
    use tari_shutdown::Shutdown;
    use tokio::sync::{mpsc, oneshot};

    use super::*;
    use crate::{handlers::test_support::TestDaemon, server::spawn_listener};

    #[test]
    fn wallet_jrpc_url_targets_json_rpc_path_and_resolves_unspecified_ips() {
        let loopback: SocketAddr = "127.0.0.1:5100".parse().unwrap();
        assert_eq!(wallet_jrpc_url(loopback), "http://127.0.0.1:5100/json_rpc");

        let unspecified_v4: SocketAddr = "0.0.0.0:5100".parse().unwrap();
        assert_eq!(wallet_jrpc_url(unspecified_v4), "http://127.0.0.1:5100/json_rpc");

        let unspecified_v6: SocketAddr = "[::]:5100".parse().unwrap();
        assert_eq!(wallet_jrpc_url(unspecified_v6), "http://[::1]:5100/json_rpc");
    }

    #[test]
    fn terminal_peer_connection_states_are_detected() {
        assert!(is_terminal_peer_connection_state(RTCPeerConnectionState::Failed));
        assert!(is_terminal_peer_connection_state(RTCPeerConnectionState::Closed));

        assert!(!is_terminal_peer_connection_state(RTCPeerConnectionState::Unspecified));
        assert!(!is_terminal_peer_connection_state(RTCPeerConnectionState::New));
        assert!(!is_terminal_peer_connection_state(RTCPeerConnectionState::Connecting));
        assert!(!is_terminal_peer_connection_state(RTCPeerConnectionState::Connected));
        assert!(!is_terminal_peer_connection_state(RTCPeerConnectionState::Disconnected));
    }

    #[tokio::test]
    async fn get_token_returns_session_permissions_token_and_ignores_caller_token() {
        let session_token = EncodedJwtString::from("scoped-delegated-jwt".to_string());
        let raw = serde_json::to_vec(&json!({
            "id": 42,
            "method": "get.token",
            "token": "caller-supplied-token",
        }))
        .unwrap();

        let response = handle_webrtc_request(&raw, &session_token, "127.0.0.1:1".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(response.id, 42);
        assert_eq!(response.payload, json!("scoped-delegated-jwt"));
    }

    #[tokio::test]
    async fn disallowed_auth_webauthn_and_webrtc_methods_are_rejected_on_data_channel() {
        let daemon = TestDaemon::start_with_transaction_service().await;
        let (listen_addr, _server) = spawn_listener(
            "127.0.0.1:0".parse().unwrap(),
            "127.0.0.1:9100".parse().unwrap(),
            daemon.context.clone(),
        )
        .await
        .unwrap();
        let session_token = EncodedJwtString::from(daemon.delegated_session("admin").token().to_string());

        for method in [
            "auth.request",
            "auth.refresh",
            "auth.revoke",
            "auth.list_sessions",
            "auth.method",
            "auth.create_api_key",
            "auth.list_api_keys",
            "auth.revoke_api_key",
            "webauthn.already_registered",
            "webauthn.reg_start",
            "webauthn.auth_start",
            "webrtc.start",
        ] {
            let raw = serde_json::to_vec(&json!({
                "id": 1,
                "method": method,
                "params": {
                    "permissions": ["admin"],
                    "credentials": "None",
                },
            }))
            .unwrap();
            let response = handle_webrtc_request(&raw, &session_token, listen_addr).await.unwrap();
            let err = response.payload["error"].as_str().unwrap_or_default();
            assert!(
                err.contains("not permitted over WebRTC"),
                "expected '{method}' to be blocked over WebRTC, got: {}",
                response.payload
            );
        }
    }

    #[tokio::test]
    async fn webrtc_request_enforces_session_permissions_token_over_caller_supplied_admin_token() {
        let daemon = TestDaemon::start_with_transaction_service().await;
        let (listen_addr, _server) = spawn_listener(
            "127.0.0.1:0".parse().unwrap(),
            "127.0.0.1:9100".parse().unwrap(),
            daemon.context.clone(),
        )
        .await
        .unwrap();

        let read_only_token = EncodedJwtString::from(daemon.delegated_session("accounts:read").token().to_string());
        let interactive_admin_token = daemon.session.token().to_string();

        // Allowed read succeeds using the session's scoped permissions_token.
        let list_raw = serde_json::to_vec(&json!({
            "id": 10,
            "method": "accounts.list",
            "params": { "offset": 0, "limit": 10 },
        }))
        .unwrap();
        let list_resp = handle_webrtc_request(&list_raw, &read_only_token, listen_addr)
            .await
            .unwrap();
        assert_eq!(list_resp.id, 10);
        assert!(list_resp.payload["accounts"].is_array(), "{}", list_resp.payload);

        // Supplying an interactive Admin JWT in `token` cannot escalate beyond `read_only_token`.
        let create_raw = serde_json::to_vec(&json!({
            "id": 11,
            "method": "accounts.create",
            "params": { "account_name": "escalated" },
            "token": interactive_admin_token,
        }))
        .unwrap();
        let create_resp = handle_webrtc_request(&create_raw, &read_only_token, listen_addr)
            .await
            .unwrap();
        assert_eq!(create_resp.id, 11);
        let create_err = create_resp.payload["error"].as_str().unwrap_or_default();
        assert!(
            create_err.contains("401") && create_err.contains("Insufficient permissions"),
            "expected 401 Insufficient permissions error, got: {}",
            create_resp.payload
        );

        // Even a delegated Admin session token is rejected by interactive-user-only endpoints.
        let delegated_admin_token = EncodedJwtString::from(daemon.delegated_session("admin").token().to_string());
        let approve_raw = serde_json::to_vec(&json!({
            "id": 12,
            "method": "signing_requests.approve",
            "params": { "request_id": 1 },
            "token": daemon.session.token(),
        }))
        .unwrap();
        let approve_resp = handle_webrtc_request(&approve_raw, &delegated_admin_token, listen_addr)
            .await
            .unwrap();
        let approve_err = approve_resp.payload["error"].as_str().unwrap_or_default();
        assert!(
            approve_err.contains("requires an interactive user session"),
            "expected interactive session check to reject delegated token, got: {}",
            approve_resp.payload
        );
    }

    #[derive(Default, Clone)]
    struct MockSignalingState {
        inner: Arc<Mutex<MockSignalingInner>>,
        answer_ready: Arc<Notify>,
    }

    #[derive(Default)]
    struct MockSignalingInner {
        offer_sdp: String,
        offer_ices: Vec<RTCIceCandidateInit>,
        answer_sdp: Option<String>,
        answer_ices: Vec<RTCIceCandidateInit>,
    }

    async fn mock_signaling_handler(
        State(state): State<MockSignalingState>,
        Json(req): Json<JsonRpcRequest>,
    ) -> Json<JsonRpcResponse> {
        let id = req.id.clone();
        let result = match req.method.as_str() {
            "get.offer" => json!(state.inner.lock().unwrap().offer_sdp.clone()),
            "get.offer_ice_candidates" => serde_json::to_value(state.inner.lock().unwrap().offer_ices.clone()).unwrap(),
            "add.answer" => {
                let sdp = req.params.as_str().unwrap().to_string();
                state.inner.lock().unwrap().answer_sdp = Some(sdp);
                state.answer_ready.notify_one();
                json!(null)
            },
            "add.answer_ice_candidate" => {
                let candidate: RTCIceCandidateInit = serde_json::from_value(req.params).unwrap();
                state.inner.lock().unwrap().answer_ices.push(candidate);
                json!(null)
            },
            other => panic!("unexpected signaling method: {other}"),
        };
        Json(JsonRpcResponse::success(id, result))
    }

    async fn spawn_mock_signaling_server() -> (SocketAddr, MockSignalingState, tokio::task::JoinHandle<()>) {
        let state = MockSignalingState::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = Router::new()
            .route("/", post(mock_signaling_handler))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            drop(axum::serve(listener, router).await);
        });
        (addr, state, task)
    }

    async fn create_loopback_offerer(
        signaling_state: &MockSignalingState,
    ) -> (
        RTCPeerConnection,
        Arc<RTCDataChannel>,
        oneshot::Receiver<()>,
        mpsc::UnboundedReceiver<Response>,
    ) {
        crate::install_default_crypto_provider();
        let client_api = APIBuilder::new().build();
        let client_pc = client_api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        let dc = client_pc.create_data_channel("tari", None).await.unwrap();

        let (dc_open_tx, dc_open_rx) = oneshot::channel::<()>();
        let dc_open_tx = Arc::new(Mutex::new(Some(dc_open_tx)));
        dc.on_open(Box::new(move || {
            if let Some(tx) = dc_open_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            Box::pin(async {})
        }));

        let (reply_tx, reply_rx) = mpsc::unbounded_channel::<Response>();
        dc.on_message(Box::new(move |msg: DataChannelMessage| {
            let parsed: Response = serde_json::from_slice(&msg.data).unwrap();
            drop(reply_tx.send(parsed));
            Box::pin(async {})
        }));

        let gather_done = Arc::new(Notify::new());
        let gather_done_hook = gather_done.clone();
        let offer_state = signaling_state.clone();
        client_pc.on_ice_candidate(Box::new(move |cand: Option<RTCIceCandidate>| {
            if let Some(c) = cand {
                if let Ok(json_cand) = c.to_json() {
                    offer_state.inner.lock().unwrap().offer_ices.push(json_cand);
                }
            } else {
                gather_done_hook.notify_one();
            }
            Box::pin(async {})
        }));

        let offer = client_pc.create_offer(None).await.unwrap();
        client_pc.set_local_description(offer.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), gather_done.notified())
            .await
            .expect("client ICE gathering timed out");
        signaling_state.inner.lock().unwrap().offer_sdp = offer.sdp;

        (client_pc, dc, dc_open_rx, reply_rx)
    }

    #[tokio::test]
    async fn webrtc_session_routes_to_bound_fallback_port_and_closes_when_peer_closes() {
        let daemon = TestDaemon::start_with_transaction_service().await;
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let occupied_addr = occupied.local_addr().unwrap();
        let (signaling_addr, signaling_state, _signaling_task) = spawn_mock_signaling_server().await;

        let (listen_addr, _wallet_server) = spawn_listener(occupied_addr, signaling_addr, daemon.context.clone())
            .await
            .unwrap();
        assert_ne!(listen_addr, occupied_addr);

        let (client_pc, dc, dc_open_rx, mut reply_rx) = create_loopback_offerer(&signaling_state).await;

        let session_shutdown = Shutdown::new();
        let read_only_token = EncodedJwtString::from(daemon.delegated_session("accounts:read").token().to_string());
        let session_handle = tokio::spawn(webrtc_start_session_with_config(
            "sig-token".to_string(),
            read_only_token,
            listen_addr,
            signaling_addr,
            session_shutdown.to_signal(),
            RTCConfiguration::default(),
        ));

        tokio::time::timeout(Duration::from_secs(5), signaling_state.answer_ready.notified())
            .await
            .expect("server answer timed out");
        let answer_sdp = signaling_state.inner.lock().unwrap().answer_sdp.clone().unwrap();
        client_pc
            .set_remote_description(RTCSessionDescription::answer(answer_sdp).unwrap())
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        let answer_ices = signaling_state.inner.lock().unwrap().answer_ices.clone();
        for cand in answer_ices {
            client_pc.add_ice_candidate(cand).await.unwrap();
        }

        tokio::time::timeout(Duration::from_secs(5), dc_open_rx)
            .await
            .expect("data channel open timed out")
            .unwrap();

        dc.send_text(
            serde_json::to_string(&json!({
                "id": 1,
                "method": "accounts.list",
                "params": { "offset": 0, "limit": 10 },
                "token": daemon.session.token(),
            }))
            .unwrap(),
        )
        .await
        .unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(5), reply_rx.recv())
            .await
            .expect("RPC reply timed out")
            .unwrap();
        assert_eq!(reply.id, 1);
        assert!(reply.payload["accounts"].is_array(), "{}", reply.payload);

        client_pc.close().await.unwrap();

        let session_res = tokio::time::timeout(Duration::from_secs(5), session_handle)
            .await
            .expect("webrtc_start_session leaked after remote peer closed")
            .unwrap();
        session_res.unwrap();
        assert!(!session_shutdown.is_triggered());
    }
}
