-- SQLite doesn't support DROP COLUMN before 3.35.0, so we recreate the table
CREATE TABLE templates_backup AS SELECT
    id,
    template_name,
    expected_hash,
    template_address,
    url,
    epoch,
    template_type,
    author_public_key,
    code,
    status,
    added_at
FROM templates;

DROP TABLE templates;

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
    added_at          timestamp                         NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO templates SELECT * FROM templates_backup;
DROP TABLE templates_backup;

CREATE UNIQUE INDEX templates_template_address_index ON templates (template_address);
