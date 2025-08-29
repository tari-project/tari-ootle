// @generated automatically by Diesel CLI.

diesel::table! {
    accounts (id) {
        id -> Integer,
        name -> Nullable<Text>,
        address -> Text,
        owner_key_index -> BigInt,
        is_default -> Bool,
        is_confirmed_on_chain -> Bool,
        stealth_resources -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    auth_status (id) {
        id -> Integer,
        user_decided -> Bool,
        granted -> Bool,
        token -> Nullable<Text>,
        revoked -> Bool,
    }
}

diesel::table! {
    authored_templates (id) {
        id -> Integer,
        author_public_key -> Text,
        address -> Text,
        name -> Text,
        tari_version -> Text,
        functions -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    config (id) {
        id -> Integer,
        key -> Text,
        value -> Text,
        is_encrypted -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    key_manager_states (id) {
        id -> Integer,
        branch_seed -> Text,
        index -> BigInt,
        is_active -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    locks (id) {
        id -> Integer,
        transaction_id -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    non_fungible_tokens (id) {
        id -> Integer,
        vault_id -> Integer,
        nft_id -> Text,
        resource_id -> Text,
        data -> Text,
        mutable_data -> Text,
        is_burnt -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    outputs (id) {
        id -> Integer,
        account_id -> Integer,
        vault_id -> Integer,
        commitment -> Text,
        value -> BigInt,
        sender_public_nonce -> Nullable<Text>,
        encryption_secret_key_index -> BigInt,
        public_asset_tag -> Nullable<Text>,
        status -> Text,
        locked_at -> Nullable<Timestamp>,
        lock_id -> Nullable<Integer>,
        encrypted_data -> Binary,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    resources (id) {
        id -> Integer,
        address -> Text,
        resource_type -> Text,
        owner_key -> Nullable<Text>,
        owner_rule -> Text,
        access_rules -> Text,
        token_symbol -> Nullable<Text>,
        divisibility -> Integer,
        metadata -> Text,
        total_supply -> Nullable<Text>,
        view_key -> Nullable<Text>,
        auth_hook -> Nullable<Text>,
        updated_at -> Timestamp,
        created_at -> Timestamp,
    }
}

diesel::table! {
    shard_state_versions (id) {
        id -> Integer,
        account_id -> Integer,
        resource_id -> Integer,
        shard -> Integer,
        state_version -> BigInt,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    stealth_outputs (id) {
        id -> Integer,
        owner_account_id -> Integer,
        resource_address -> Text,
        commitment -> Text,
        value -> Text,
        sender_public_nonce -> Text,
        status -> Text,
        locked_at -> Nullable<Timestamp>,
        lock_id -> Nullable<Integer>,
        encryption_secret_key_index -> BigInt,
        encrypted_data -> Binary,
        tag_byte -> Integer,
        is_burnt -> Bool,
        is_frozen -> Bool,
        is_on_chain -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    substates (id) {
        id -> Integer,
        module_name -> Nullable<Text>,
        address -> Text,
        parent_address -> Nullable<Text>,
        referenced_substates -> Text,
        version -> Integer,
        template_address -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    transactions (id) {
        id -> Integer,
        transaction_id -> Text,
        transaction_json -> Text,
        referenced_components -> Text,
        signers -> Text,
        result -> Nullable<Text>,
        qcs -> Nullable<Text>,
        final_fee -> Nullable<BigInt>,
        status -> Text,
        dry_run -> Bool,
        executed_time_ms -> Nullable<BigInt>,
        finalized_time -> Nullable<Timestamp>,
        new_account_info -> Nullable<Text>,
        invalid_reason -> Nullable<Text>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    vaults (id) {
        id -> Integer,
        account_id -> Integer,
        address -> Text,
        resource_address -> Text,
        resource_type -> Text,
        revealed_balance -> BigInt,
        confidential_balance -> BigInt,
        locked_revealed_balance -> BigInt,
        token_symbol -> Nullable<Text>,
        divisibility -> Integer,
        locked_by -> Nullable<Integer>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    webauthn_registration_passkeys (id) {
        id -> Integer,
        registration_id -> Integer,
        passkey -> Binary,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    webauthn_registrations (id) {
        id -> Integer,
        username -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::joinable!(non_fungible_tokens -> vaults (vault_id));
diesel::joinable!(outputs -> accounts (account_id));
diesel::joinable!(outputs -> vaults (vault_id));
diesel::joinable!(shard_state_versions -> accounts (account_id));
diesel::joinable!(shard_state_versions -> resources (resource_id));
diesel::joinable!(vaults -> accounts (account_id));
diesel::joinable!(vaults -> locks (locked_by));
diesel::joinable!(webauthn_registration_passkeys -> webauthn_registrations (registration_id));

diesel::allow_tables_to_appear_in_same_query!(
    accounts,
    auth_status,
    authored_templates,
    config,
    key_manager_states,
    locks,
    non_fungible_tokens,
    outputs,
    resources,
    shard_state_versions,
    stealth_outputs,
    substates,
    transactions,
    vaults,
    webauthn_registration_passkeys,
    webauthn_registrations,
);
