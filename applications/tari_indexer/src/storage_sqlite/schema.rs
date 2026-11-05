// @generated automatically by Diesel CLI.

diesel::table! {
    template_catalogue (id) {
        id -> BigInt,
        template_address -> Text,
        template_name -> Text,
        author_public_key -> Text,
        binary_hash -> Text,
        at_epoch -> BigInt,
        metadata_hash -> Nullable<Text>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    epoch_checkpoints (id) {
        id -> BigInt,
        epoch -> BigInt,
        shard_group -> Text,
        json_data -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        shard_start -> Integer,
        shard_end -> Integer,
    }
}

diesel::table! {
    events (id) {
        id -> BigInt,
        template_address -> Text,
        tx_hash -> Text,
        topic -> Text,
        event -> Binary,
        substate_id -> Nullable<Text>,
        resource_address -> Nullable<Text>,
        created_at -> Timestamp,
        epoch -> BigInt,
    }
}

diesel::table! {
    key_values (id) {
        id -> BigInt,
        key -> Text,
        value -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    checkpoint_shard_leaves (shard, state_version) {
        shard -> Integer,
        state_version -> BigInt,
        transition_acc -> Binary,
    }
}

diesel::table! {
    snapshot_substates (shard, epoch, leaf_key) {
        shard -> Integer,
        epoch -> BigInt,
        leaf_key -> Binary,
        substate -> Binary,
    }
}

diesel::table! {
    staged_versions (shard, state_version) {
        shard -> Integer,
        state_version -> BigInt,
        epoch -> BigInt,
        transition_acc -> Binary,
        streamed_by -> Binary,
        batch -> Binary,
    }
}

diesel::table! {
    substates (id) {
        id -> BigInt,
        address -> Text,
        version -> BigInt,
        data -> Binary,
        template_address -> Nullable<Text>,
        module_name -> Nullable<Text>,
        updated_at -> Timestamp,
        created_at -> Timestamp,
    }
}

diesel::table! {
    transaction_receipts (id) {
        id -> BigInt,
        address -> Text,
        data -> Binary,
        created_at -> Timestamp,
        outcome -> Text,
        total_fees_paid -> BigInt,
        epoch -> BigInt,
    }
}

diesel::table! {
    transactions (id) {
        id -> BigInt,
        transaction_id -> Text,
        body -> Text,
        created_at -> Timestamp,
        rejected_reason -> Nullable<Text>,
        rejected_at -> Nullable<Timestamp>,
        retention_epoch -> BigInt,
        source -> Text,
    }
}

diesel::table! {
    watched_substates (id) {
        id -> BigInt,
        component_address -> Text,
        template_address -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    utxos (id) {
        id -> BigInt,
        commitment -> Text,
        public_nonce -> Nullable<Text>,
        version -> BigInt,
        resource_address -> Text,
        shard -> Integer,
        state_version -> BigInt,
        utxo -> Nullable<Binary>,
        utxo_tag -> Nullable<Integer>,
        epoch -> BigInt,
        is_spent -> Bool,
        is_burnt -> Bool,
        created_at -> Timestamp,
    }
}

diesel::table! {
    utxo_tombstone_horizons (shard) {
        shard -> Integer,
        state_version -> BigInt,
    }
}

diesel::table! {
    verified_state_roots (id) {
        id -> BigInt,
        epoch -> BigInt,
        shard_group -> Text,
        block_height -> BigInt,
        block_hash -> Text,
        state_merkle_root -> Text,
        validated_at -> Timestamp,
    }
}

diesel::table! {
    substate_cache (substate_id) {
        substate_id -> Text,
        version -> Nullable<BigInt>,
        verified -> Bool,
        substate_result -> Binary,
        cached_at -> BigInt,
    }
}

diesel::table! {
    substate_cache_proofs (substate_id, version) {
        substate_id -> Text,
        version -> BigInt,
        value_proof -> Binary,
        commit_proof -> Binary,
        proof_epoch -> BigInt,
    }
}

diesel::table! {
    substate_cache_invalidations (substate_id) {
        substate_id -> Text,
        state_version -> BigInt,
        substate_version -> BigInt,
        spent -> Bool,
        invalidated_at -> BigInt,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    checkpoint_shard_leaves,
    epoch_checkpoints,
    events,
    key_values,
    substate_cache,
    substate_cache_invalidations,
    substate_cache_proofs,
    snapshot_substates,
    staged_versions,
    substates,
    template_catalogue,
    transaction_receipts,
    transactions,
    utxo_tombstone_horizons,
    utxos,
    verified_state_roots,
    watched_substates,
);
