CREATE TABLE webauthn_registrations
(
    id           INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    username     TEXT NOT NULL,
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX webauthn_regs_usernames ON webauthn_registrations (username);

CREATE TABLE webauthn_registration_passkeys
(
    id           INTEGER  NOT NULL PRIMARY KEY AUTOINCREMENT,
    registration_id     INTEGER NOT NULL REFERENCES webauthn_registrations (id),
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);