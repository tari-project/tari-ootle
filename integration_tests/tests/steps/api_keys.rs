//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use cucumber::{gherkin::Step, given, then, when};
use integration_tests::{TariWorld, cucumber_log, wallet_daemon_client};
use reqwest::Url;
use tari_ootle_walletd_client::{
    ComponentAddressOrName, WalletDaemonClient,
    error::WalletDaemonClientError,
    permissions::JrpcPermission,
    types::{AccountsTransferRequest, AuthCredentials, AuthLoginRequest},
};
use tari_template_lib_types::{Amount, constants::TARI_TOKEN};

const TEST_WALLET_DAEMON: &str = "WALLET_D";
const TEST_ACCOUNT: &str = "API_KEY_ACCOUNT";

#[given(expr = "the user is authenticated as admin")]
async fn given_the_user_is_authenticated_as_admin(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    reset_api_key_state(world);
    world.api_key_test_state.session_permissions = vec![JrpcPermission::Admin];
}

#[given(expr = "the user is authenticated as non-admin")]
async fn given_the_user_is_authenticated_as_non_admin(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    reset_api_key_state(world);
    world.api_key_test_state.session_permissions = vec![JrpcPermission::AccountInfo];
}

#[when(regex = r#"^the admin creates an API key named "([^"]+)" with scopes (\[[^\]]+\])$"#)]
async fn when_the_admin_creates_an_api_key_named_with_scopes(
    world: &mut TariWorld,
    step: &Step,
    name: String,
    scopes: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let permissions = parse_permissions(&scopes);
    create_api_key_as_admin(world, name, permissions, false).await;
}

#[then(expr = "the response contains a plaintext key starting with {string}")]
async fn then_the_response_contains_a_plaintext_key_starting_with(world: &mut TariWorld, step: &Step, prefix: String) {
    cucumber_log!("==== Step: {}", step.value);
    let plaintext = world
        .api_key_test_state
        .plaintext_key
        .as_deref()
        .expect("Expected a plaintext API key to be present");
    assert!(
        plaintext.starts_with(&prefix),
        "Expected plaintext API key to start with {}, got {}",
        prefix,
        plaintext
    );
}

#[then(expr = "the API key list contains a key named {string}")]
async fn then_the_api_key_list_contains_a_key_named(world: &mut TariWorld, step: &Step, name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let mut client = admin_client(world).await;
    let response = client.list_api_keys().await.unwrap();
    assert!(
        response.keys.iter().any(|k| k.name == name),
        "Expected API key list to contain {}, got {:?}",
        name,
        response.keys
    );
    world.api_key_test_state.last_list_response = Some(response);
}

#[then(expr = "the plaintext key is not in the list response")]
async fn then_the_plaintext_key_is_not_in_the_list_response(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let plaintext = world
        .api_key_test_state
        .plaintext_key
        .as_deref()
        .expect("Expected plaintext API key to be present");
    let response = world
        .api_key_test_state
        .last_list_response
        .as_ref()
        .expect("Expected API key list response to be present");
    assert!(
        response
            .keys
            .iter()
            .all(|key| key.id != plaintext && key.name != plaintext),
        "Plaintext API key unexpectedly appeared in list response"
    );
}

#[when(expr = "the non-admin attempts to create an API key")]
async fn when_the_non_admin_attempts_to_create_an_api_key(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let mut client = session_client(world).await;
    match client
        .create_api_key("non-admin-agent", vec![JrpcPermission::AccountInfo], false)
        .await
    {
        Ok(response) => panic!("Expected permission error, got success: {:?}", response),
        Err(err) => store_client_error(world, err),
    }
}

#[when(expr = "the non-admin attempts to list API keys")]
async fn when_the_non_admin_attempts_to_list_api_keys(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let mut client = session_client(world).await;
    match client.list_api_keys().await {
        Ok(response) => panic!("Expected permission error, got success: {:?}", response),
        Err(err) => store_client_error(world, err),
    }
}

#[then(expr = "the response is a permission denied error")]
async fn then_the_response_is_a_permission_denied_error(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    assert_eq!(
        world.api_key_test_state.last_error_code,
        Some(401),
        "Expected unauthorized error, got {:?}: {:?}",
        world.api_key_test_state.last_error_code,
        world.api_key_test_state.last_error_message
    );
}

#[given(regex = r#"^the admin has created an API key with scopes (\[[^\]]+\])$"#)]
async fn given_the_admin_has_created_an_api_key_with_scopes(world: &mut TariWorld, step: &Step, scopes: String) {
    cucumber_log!("==== Step: {}", step.value);
    let permissions = parse_permissions(&scopes);
    create_api_key_as_admin(world, "scoped-agent".to_string(), permissions, false).await;
}

#[given(regex = r#"^the admin has created an API key with scopes (\[[^\]]+\]) only$"#)]
async fn given_the_admin_has_created_an_api_key_with_scopes_only(world: &mut TariWorld, step: &Step, scopes: String) {
    cucumber_log!("==== Step: {}", step.value);
    let permissions = parse_permissions(&scopes);
    create_api_key_as_admin(world, "scoped-agent-only".to_string(), permissions, false).await;
}

#[given(expr = "the admin has created an API key")]
async fn given_the_admin_has_created_an_api_key(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    create_api_key_as_admin(
        world,
        "revocation-agent".to_string(),
        vec![JrpcPermission::AccountInfo],
        false,
    )
    .await;
}

#[when(expr = "a new client authenticates using the API key")]
async fn when_a_new_client_authenticates_using_the_api_key(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let raw_key = world
        .api_key_test_state
        .plaintext_key
        .clone()
        .expect("Expected plaintext API key to be present");
    let mut client = new_wallet_daemon_client(world);
    let response = client.authenticate_with_api_key(&raw_key).await.unwrap();
    world.api_key_test_state.last_auth_token = Some(response.token.as_str().to_string());
}

#[then(expr = "authentication succeeds and a JWT is returned")]
async fn then_authentication_succeeds_and_a_jwt_is_returned(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let token = world
        .api_key_test_state
        .last_auth_token
        .as_deref()
        .expect("Expected an authentication token to be present");
    assert!(
        !token.is_empty() && token.split('.').count() == 3,
        "Expected a JWT token, got {}",
        token
    );
}

#[then(expr = "the agent can call accounts.get_default successfully")]
async fn then_the_agent_can_call_accounts_get_default_successfully(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let token = world
        .api_key_test_state
        .last_auth_token
        .clone()
        .expect("Expected an authentication token to be present");
    let mut client = new_wallet_daemon_client(world);
    client.set_auth_token(token.into());
    let response = client.accounts_get_default().await.unwrap();
    assert_eq!(response.account.name.as_deref(), Some(TEST_ACCOUNT));
}

#[when(expr = "the agent authenticates and calls a transfer method")]
async fn when_the_agent_authenticates_and_calls_a_transfer_method(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let raw_key = world
        .api_key_test_state
        .plaintext_key
        .clone()
        .expect("Expected plaintext API key to be present");
    let destination_public_key = world
        .wallet_accounts
        .get(TEST_ACCOUNT)
        .expect("Expected the test account to exist")
        .account
        .owner_public_key();

    let mut client = new_wallet_daemon_client(world);
    client.authenticate_with_api_key(&raw_key).await.unwrap();

    match client
        .accounts_transfer(AccountsTransferRequest {
            account: Some(ComponentAddressOrName::Name(TEST_ACCOUNT.to_string())),
            amount: Amount::from(1),
            resource_address: TARI_TOKEN,
            destination_public_key: destination_public_key.clone(),
            max_fee: 1000,
            proof_from_badge_resource: None,
            dry_run: false,
        })
        .await
    {
        Ok(response) => panic!("Expected permission error, got success: {:?}", response),
        Err(err) => store_client_error(world, err),
    }
}

#[when(expr = "the admin revokes the API key")]
async fn when_the_admin_revokes_the_api_key(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let id = world
        .api_key_test_state
        .created_key
        .as_ref()
        .expect("Expected API key create response to be present")
        .id
        .clone();
    let mut client = admin_client(world).await;
    client.revoke_api_key(id).await.unwrap();
}

#[when(expr = "a client attempts to authenticate with the revoked key")]
async fn when_a_client_attempts_to_authenticate_with_the_revoked_key(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    let raw_key = world
        .api_key_test_state
        .plaintext_key
        .clone()
        .expect("Expected plaintext API key to be present");
    let mut client = new_wallet_daemon_client(world);
    match client.authenticate_with_api_key(&raw_key).await {
        Ok(response) => panic!("Expected authentication rejection, got success: {:?}", response),
        Err(err) => store_client_error(world, err),
    }
}

#[then(expr = "the authentication is rejected")]
async fn then_the_authentication_is_rejected(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    assert_eq!(
        world.api_key_test_state.last_error_code,
        Some(401),
        "Expected authentication rejection, got {:?}: {:?}",
        world.api_key_test_state.last_error_code,
        world.api_key_test_state.last_error_message
    );
}

#[when(expr = "the admin creates an API key with Admin scope and grant_admin false")]
async fn when_the_admin_creates_an_api_key_with_admin_scope_and_grant_admin_false(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    ensure_test_account_exists(world).await;
    let mut client = admin_client(world).await;
    match client
        .create_api_key("admin-agent", vec![JrpcPermission::Admin], false)
        .await
    {
        Ok(response) => panic!("Expected confirmation error, got success: {:?}", response),
        Err(err) => store_client_error(world, err),
    }
}

#[then(expr = "the response is an AdminScopeRequiresConfirmation error")]
async fn then_the_response_is_an_admin_scope_requires_confirmation_error(world: &mut TariWorld, step: &Step) {
    cucumber_log!("==== Step: {}", step.value);
    assert_eq!(
        world.api_key_test_state.last_error_code,
        Some(-32003),
        "Expected AdminScopeRequiresConfirmation error, got {:?}: {:?}",
        world.api_key_test_state.last_error_code,
        world.api_key_test_state.last_error_message
    );
}

fn reset_api_key_state(world: &mut TariWorld) {
    world.api_key_test_state.plaintext_key = None;
    world.api_key_test_state.created_key = None;
    world.api_key_test_state.last_list_response = None;
    world.api_key_test_state.last_error_code = None;
    world.api_key_test_state.last_error_message = None;
    world.api_key_test_state.last_auth_token = None;
}

fn parse_permissions(scopes: &str) -> Vec<JrpcPermission> {
    serde_json::from_str::<Vec<String>>(scopes)
        .unwrap_or_else(|e| panic!("Failed to parse permission list {scopes}: {e}"))
        .into_iter()
        .map(|permission| JrpcPermission::from_str(&permission).unwrap())
        .collect()
}

fn new_wallet_daemon_client(world: &TariWorld) -> WalletDaemonClient {
    let daemon = world.get_wallet_daemon(TEST_WALLET_DAEMON);
    let endpoint = Url::parse(&format!("http://127.0.0.1:{}/json_rpc", daemon.json_rpc_port)).unwrap();
    WalletDaemonClient::connect(endpoint, None).unwrap()
}

async fn session_client(world: &TariWorld) -> WalletDaemonClient {
    auth_client_with_permissions(world, world.api_key_test_state.session_permissions.clone()).await
}

async fn admin_client(world: &TariWorld) -> WalletDaemonClient {
    auth_client_with_permissions(world, vec![JrpcPermission::Admin]).await
}

async fn auth_client_with_permissions(world: &TariWorld, permissions: Vec<JrpcPermission>) -> WalletDaemonClient {
    let mut client = new_wallet_daemon_client(world);
    let response = client
        .auth_request(AuthLoginRequest {
            permissions,
            credentials: AuthCredentials::None,
        })
        .await
        .unwrap();
    client.set_auth_token(response.token);
    client
}

async fn ensure_test_account_exists(world: &mut TariWorld) {
    if world.wallet_accounts.contains_key(TEST_ACCOUNT) {
        return;
    }
    wallet_daemon_client::create_account(world, TEST_WALLET_DAEMON.to_string(), TEST_ACCOUNT.to_string()).await;
}

async fn create_api_key_as_admin(
    world: &mut TariWorld,
    name: String,
    permissions: Vec<JrpcPermission>,
    grant_admin: bool,
) {
    ensure_test_account_exists(world).await;
    let mut client = admin_client(world).await;
    let response = client.create_api_key(name, permissions, grant_admin).await.unwrap();
    reset_error(world);
    world.api_key_test_state.plaintext_key = Some(response.key.clone());
    world.api_key_test_state.created_key = Some(response);
    world.api_key_test_state.last_auth_token = None;
}

fn store_client_error(world: &mut TariWorld, error: WalletDaemonClientError) {
    let state = &mut world.api_key_test_state;
    match error {
        WalletDaemonClientError::Unauthorized { message } => {
            state.last_error_code = Some(401);
            state.last_error_message = Some(message);
        },
        WalletDaemonClientError::RequestFailedWithStatus { code, message } => {
            state.last_error_code = Some(code);
            state.last_error_message = Some(message);
        },
        other => panic!("Unexpected wallet daemon client error: {other}"),
    }
}

fn reset_error(world: &mut TariWorld) {
    world.api_key_test_state.last_error_code = None;
    world.api_key_test_state.last_error_message = None;
}
