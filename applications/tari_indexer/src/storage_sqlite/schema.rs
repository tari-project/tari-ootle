// @generated automatically by Diesel CLI.

diesel::table! {
    epoch_checkpoints (id) {
        id -> Integer,
        epoch -> BigInt,
        shard_group -> Text,
        json_data -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    events (id) {
        id -> Integer,
        template_address -> Text,
        tx_hash -> Text,
        topic -> Text,
        payload -> Text,
        substate_id -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    key_values (id) {
        id -> Integer,
        key -> Text,
        value -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    substate_transitions (id) {
        id -> Integer,
        shard -> Integer,
        state_version -> BigInt,
        epoch -> BigInt,
        substate_id -> Text,
        version -> Integer,
        substate_type -> Text,
        is_up -> Bool,
        value_hash -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    substates (id) {
        id -> Integer,
        address -> Text,
        version -> Integer,
        data -> Text,
        template_address -> Nullable<Text>,
        module_name -> Nullable<Text>,
        updated_at -> Timestamp,
        created_at -> Timestamp,
    }
}

diesel::table! {
    transaction_receipts (id) {
        id -> Integer,
        address -> Text,
        data -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    transactions (id) {
        id -> Integer,
        transaction_id -> Text,
        body -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    utxos (id) {
        id -> Integer,
        commitment -> Text,
        public_nonce -> Text,
        version -> Integer,
        resource_address -> Text,
        shard -> Integer,
        state_version -> BigInt,
        output -> Nullable<Binary>,
        utxo_tag -> Integer,
        epoch -> BigInt,
        is_spent -> Bool,
        is_burnt -> Bool,
        is_frozen -> Bool,
        created_at -> Timestamp,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    epoch_checkpoints,
    events,
    key_values,
    substate_transitions,
    substates,
    transaction_receipts,
    transactions,
    utxos,
);
