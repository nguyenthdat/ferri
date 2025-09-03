-- Add migration script here
-- One account can have many external identities (Google, GitHub, …)
CREATE TABLE account_identities (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    provider        TEXT    NOT NULL,     -- 'google', 'github', 'azuread', …
    subject         TEXT    NOT NULL,     -- stable provider user id (OIDC 'sub')
    email           TEXT,
    email_verified  INTEGER NOT NULL DEFAULT 0 CHECK (email_verified IN (0,1)),
    display_name    TEXT,
    avatar_url      TEXT,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(provider, subject)
);
CREATE INDEX idx_account_identities_account ON account_identities(account_id);

CREATE TRIGGER trg_account_identities_touch_updated
AFTER UPDATE ON account_identities
FOR EACH ROW BEGIN
  UPDATE account_identities SET updated_at = strftime('%s','now') WHERE id = OLD.id;
END;

-- Ephemeral state for the auth code flow with PKCE
CREATE TABLE oauth_states (
    state         TEXT PRIMARY KEY,        -- random
    code_verifier TEXT NOT NULL,           -- store as is or HMAC-hash if you prefer
    nonce         TEXT,                    -- for OIDC ID token replay protection
    redirect_uri  TEXT NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    expires_at    INTEGER NOT NULL,        -- now()+600 for ~10 min window
    used          INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0,1))
);

-- App sessions (cookie-backed). Store a hash of the token, not the token itself.
CREATE TABLE sessions (
    id_hash      TEXT PRIMARY KEY,               -- e.g., base64(SHA-256(session_id))
    account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_seen_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    expires_at   INTEGER,                        -- absolute timeout
    ip           TEXT,
    user_agent   TEXT
);
CREATE INDEX idx_sessions_account ON sessions(account_id);

-- Only if you must call provider APIs on behalf of the user
CREATE TABLE provider_tokens (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    account_identity_id   INTEGER NOT NULL REFERENCES account_identities(id) ON DELETE CASCADE,
    access_token_enc      BLOB,     -- encrypted at rest
    refresh_token_enc     BLOB,     -- encrypted at rest (or hash + envelope)
    expires_at            INTEGER,
    scope                 TEXT
);