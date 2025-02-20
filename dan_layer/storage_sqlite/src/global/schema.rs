// @generated automatically by Diesel CLI.

diesel::table! {
    base_layer_block_info (hash) {
        hash -> Binary,
        height -> BigInt,
    }
}

diesel::table! {
    bmt_cache (epoch) {
        epoch -> BigInt,
        bmt -> Binary,
    }
}

diesel::table! {
    committees (id) {
        id -> Integer,
        validator_node_id -> Integer,
        epoch -> BigInt,
        shard_start -> Integer,
        shard_end -> Integer,
    }
}

diesel::table! {
    epochs (epoch) {
        epoch -> BigInt,
        validator_node_mr -> Binary,
    }
}

diesel::table! {
    layer_one_transactions (id) {
        id -> Integer,
        epoch -> BigInt,
        payload_type -> Text,
        payload -> Text,
        submitted_at -> Nullable<Timestamp>,
        is_observed -> Bool,
    }
}

diesel::table! {
    metadata (key_name) {
        key_name -> Binary,
        value -> Binary,
    }
}

diesel::table! {
    templates (id) {
        id -> Integer,
        template_name -> Text,
        expected_hash -> Binary,
        template_address -> Binary,
        url -> Nullable<Text>,
        epoch -> BigInt,
        template_type -> Text,
        author_public_key -> Binary,
        code -> Nullable<Binary>,
        status -> Text,
        added_at -> Timestamp,
    }
}

diesel::table! {
    validator_nodes (id) {
        id -> Integer,
        public_key -> Binary,
        address -> Text,
        shard_key -> Binary,
        start_epoch -> BigInt,
        end_epoch -> Nullable<BigInt>,
        fee_claim_public_key -> Binary,
    }
}

diesel::joinable!(committees -> validator_nodes (validator_node_id));

diesel::allow_tables_to_appear_in_same_query!(
    base_layer_block_info,
    bmt_cache,
    committees,
    epochs,
    layer_one_transactions,
    metadata,
    templates,
    validator_nodes,
);
