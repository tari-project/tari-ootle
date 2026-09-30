CREATE TABLE templates
(
    id                Integer primary key autoincrement not null,
    template_name     text                              not null,
    expected_hash     blob                              not null,
    template_address  blob                              not null,
    url               text                              null,
    epoch             bigint                            not null,
    template_type     text                              not null,
    author_public_key blob                              not null,
    code              blob                              null,
    status            VARCHAR(20)                       NOT NULL DEFAULT 'New',
    added_at          timestamp                         NOT NULL DEFAULT CURRENT_TIMESTAMP,
    metadata_hash     BLOB                              NULL
);

CREATE UNIQUE INDEX templates_template_address_index ON templates (template_address);
