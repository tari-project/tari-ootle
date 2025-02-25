--  // Copyright 2021. The Tari Project
--  //
--  // Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
--  // following conditions are met:
--  //
--  // 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
--  // disclaimer.
--  //
--  // 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
--  // following disclaimer in the documentation and/or other materials provided with the distribution.
--  //
--  // 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
--  // products derived from this software without specific prior written permission.
--  //
--  // THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
--  // INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
--  // DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
--  // SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
--  // SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
--  // WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
--  // USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

create table metadata
(
    key_name blob primary key not null,
    value    blob             not null
);

create table validator_nodes
(
    id                   integer primary key autoincrement not null,
    public_key           blob                              not null,
    address              text                              not null,
    shard_key            blob                              not null,
    start_epoch          bigint                            not null,
    end_epoch            bigint                            null,
    fee_claim_public_key blob                              not null
);

CREATE TABLE committees
(
    id                INTEGER PRIMARY KEY autoincrement NOT NULL,
    validator_node_id INTEGER                           NOT NULL,
    epoch             BIGINT                            NOT NULL,
    shard_start       INTEGER                           NOT NULL,
    shard_end         INTEGER                           NOT NULL,
    FOREIGN KEY (validator_node_id) REFERENCES validator_nodes (id) ON DELETE CASCADE
);

CREATE INDEX committees_validator_node_id_epoch_index ON committees (validator_node_id, epoch);


create table templates
(
    id                Integer primary key autoincrement not null,
    -- template name
    template_name     text                              not null,
    expected_hash     blob                              not null,
    -- the address is the hash of the content
    template_address  blob                              not null,
    -- where to find the template code
    url               text                              null,
    -- the epoch in which the template was published
    epoch             bigint                            not null,
    -- The type of template, used to create an enum in code
    template_type     text                              not null,
    author_public_key blob                              not null,

    -- template code for the given template type
    code              blob                              null,
    status            VARCHAR(20)                       NOT NULL DEFAULT 'New',
    added_at          timestamp                         NOT NULL DEFAULT CURRENT_TIMESTAMP
);


-- fetching by the template_address will be a very common operation
create unique index templates_template_address_index on templates (template_address);


create table epochs
(
    epoch             bigint primary key not null,
    validator_node_mr blob               not null
);

create table bmt_cache
(
    epoch bigint primary key not null,
    bmt   blob               not null
);

create table base_layer_block_info
(
    hash   blob primary key not null,
    height bigint           not null
);

CREATE TABLE layer_one_transactions
(
    id           INTEGER PRIMARY KEY autoincrement NOT NULL,
    epoch        BIGINT                            NOT NULL,
    payload_type TEXT                              NOT NULL,
    payload      TEXT                              NOT NULL,
    submitted_at DATETIME                          NULL,
    is_observed  BOOLEAN                           NOT NULL DEFAULT '0'
);

