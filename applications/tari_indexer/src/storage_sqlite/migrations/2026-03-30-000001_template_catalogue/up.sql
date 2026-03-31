-- Copyright 2025. The Tari Project
-- SPDX-License-Identifier: BSD-3-Clause

-- Stores lightweight template metadata received from validators via the TEMPLATE_METADATA sync flag.
-- This enables the indexer to serve a searchable template catalogue without storing full WASM binaries.
CREATE TABLE template_catalogue
(
    id                INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    -- Hex-encoded template address (TemplateAddress = Hash32)
    template_address  TEXT                              NOT NULL UNIQUE,
    -- Human-readable template name extracted from the WASM ABI
    template_name     TEXT                              NOT NULL,
    -- Hex-encoded author public key (RistrettoPublicKeyBytes)
    author_public_key TEXT                              NOT NULL,
    -- Hex-encoded SHA-256 hash of the WASM binary
    binary_hash       TEXT                              NOT NULL,
    -- Epoch at which the template was published
    at_epoch          BIGINT                            NOT NULL,
    created_at        TIMESTAMP                         NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at        TIMESTAMP                         NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX template_catalogue_template_name_idx ON template_catalogue (template_name);
CREATE INDEX template_catalogue_author_public_key_idx ON template_catalogue (author_public_key);
