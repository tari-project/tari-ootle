CREATE TABLE authored_templates
(
    id            INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    key_index     INT NOT NULL,
    address       TEXT NOT NULL,
    name          TEXT NOT NULL,
    tari_version  TEXT NOT NULL,
    functions     TEXT NOT NULL,
    created_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX authored_templates_key_indexes ON authored_templates (key_index);
