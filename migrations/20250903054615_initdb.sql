-- Schema for Accounts and VFS inspired by HFS config
-- SQLite dialect
--
-- Conventions
--  - Timestamps are INTEGER (Unix seconds)
--  - Booleans are INTEGER 0/1 with CHECK
--  - Complex descriptors (WhoCan, mask properties) are stored as JSON TEXT
--  - ON DELETE CASCADE everywhere it makes sense
--
-- Note: application should set PRAGMA foreign_keys = ON per-connection.

-- =============================
-- Accounts / Groups
-- =============================
CREATE TABLE accounts (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    username        TEXT    NOT NULL UNIQUE,
    is_group        INTEGER NOT NULL DEFAULT 0 CHECK (is_group IN (0,1)),
    admin           INTEGER NOT NULL DEFAULT 0 CHECK (admin IN (0,1)),
    ignore_limits   INTEGER NOT NULL DEFAULT 0 CHECK (ignore_limits IN (0,1)),
    redirect        TEXT,
    srp             TEXT,                         -- encrypted password (NULL for groups)
    disabled        INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0,1)),
    expire          INTEGER,                      -- unix seconds; NULL = no expiry
    days_to_live    INTEGER,                      -- set on first login; NULL = none
    disable_password_change INTEGER NOT NULL DEFAULT 0 CHECK (disable_password_change IN (0,1)),
    require_password_change INTEGER NOT NULL DEFAULT 0 CHECK (require_password_change IN (0,1)),
    allow_net       TEXT,                         -- network mask(s)
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE INDEX idx_accounts_username ON accounts(username);

-- Users can belong to groups (which are accounts with is_group=1)
CREATE TABLE account_memberships (
    account_id  INTEGER NOT NULL,
    group_id    INTEGER NOT NULL,
    PRIMARY KEY (account_id, group_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (group_id)   REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX idx_memberships_account ON account_memberships(account_id);
CREATE INDEX idx_memberships_group   ON account_memberships(group_id);

-- Enforce group_id refers to a group account
CREATE TRIGGER trg_memberships_group_check
BEFORE INSERT ON account_memberships
FOR EACH ROW BEGIN
    SELECT CASE WHEN (SELECT is_group FROM accounts WHERE id = NEW.group_id) != 1
        THEN RAISE(ABORT, 'group_id must reference an account with is_group=1') END;
END;

-- Touch updated_at on accounts
CREATE TRIGGER trg_accounts_touch_updated_at
AFTER UPDATE ON accounts
FOR EACH ROW BEGIN
    UPDATE accounts SET updated_at = strftime('%s','now') WHERE id = OLD.id;
END;

-- =============================
-- VFS Core
-- =============================
CREATE TABLE vfs_nodes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id       INTEGER REFERENCES vfs_nodes(id) ON DELETE CASCADE,
    name            TEXT,             -- display name; if NULL, may be inferred from source
    source_path     TEXT,             -- disk path; NULL for virtual-only or link
    url             TEXT,             -- external link; mutually exclusive with source_path in practice
    mime            TEXT,
    ord             INTEGER,          -- "order" property: positive=top, negative=bottom
    target          TEXT,             -- link target (e.g., _blank)
    accept          TEXT,             -- upload accept pattern (e.g., .zip,.rar)
    default_child_id INTEGER REFERENCES vfs_nodes(id) DEFERRABLE INITIALLY DEFERRED,
    default_child_path TEXT,          -- alternative to id when not created yet
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    CHECK (name IS NOT NULL OR source_path IS NOT NULL OR url IS NOT NULL)
);

CREATE INDEX idx_vfs_parent           ON vfs_nodes(parent_id);
CREATE INDEX idx_vfs_name_by_parent   ON vfs_nodes(parent_id, name);
CREATE INDEX idx_vfs_default_child_id ON vfs_nodes(default_child_id);

CREATE TRIGGER trg_vfs_nodes_touch_updated
AFTER UPDATE ON vfs_nodes
FOR EACH ROW BEGIN
    UPDATE vfs_nodes SET updated_at = strftime('%s','now') WHERE id = OLD.id;
END;

-- Optional explicit renames for items read from source
CREATE TABLE vfs_node_renames (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id      INTEGER NOT NULL REFERENCES vfs_nodes(id) ON DELETE CASCADE,
    original_name TEXT NOT NULL,
    new_name      TEXT NOT NULL,
    UNIQUE(node_id, original_name)
);

-- Node-level permissions; WhoCan descriptors stored as JSON/text
-- permission in {can_read, can_see, can_upload, can_list, can_archive, can_delete}
-- who is one of: true, false, "*", ["user1","user2"], "can_SOMETHING", or {"this": WhoCan, "children": WhoCan}
CREATE TABLE vfs_node_permissions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id     INTEGER NOT NULL REFERENCES vfs_nodes(id) ON DELETE CASCADE,
    permission  TEXT    NOT NULL CHECK (permission IN (
                    'can_read','can_see','can_upload','can_list','can_archive','can_delete'
                )),
    who         TEXT    NOT NULL,
    UNIQUE(node_id, permission)
);

-- Masks that map glob patterns to sets of properties; stored as JSON
-- Example properties object can carry any node property (even nested "masks")
CREATE TABLE vfs_node_masks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id     INTEGER NOT NULL REFERENCES vfs_nodes(id) ON DELETE CASCADE,
    mask        TEXT    NOT NULL,
    properties  TEXT    NOT NULL,  -- JSON object with keys like can_read, mime, etc.
    ord         INTEGER,           -- masks on top have priority over bottom rules
    UNIQUE(node_id, mask)
);

CREATE INDEX idx_vfs_masks_node ON vfs_node_masks(node_id);

-- Roots map host (or mask) to a specific VFS node
CREATE TABLE vfs_roots (
    host_mask TEXT PRIMARY KEY,
    node_id   INTEGER NOT NULL REFERENCES vfs_nodes(id) ON DELETE CASCADE
);
