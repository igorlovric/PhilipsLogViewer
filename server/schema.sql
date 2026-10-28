-- Philips Log Viewer – shared pattern library
-- mysql -u root -p plv < schema.sql

CREATE TABLE IF NOT EXISTS plv_patterns (
    uid         CHAR(36)        NOT NULL,
    owner_hash  CHAR(64)        NOT NULL COMMENT 'sha256 of the owner key of the installation that created it',
    author      VARCHAR(100)    NOT NULL DEFAULT '',
    name        VARCHAR(200)    NOT NULL DEFAULT '',
    checksum    CHAR(64)        NOT NULL COMMENT 'sha256 of data',
    data        MEDIUMTEXT      NOT NULL COMMENT 'pattern JSON exactly as sent by the program',
    rev         BIGINT UNSIGNED NOT NULL COMMENT 'global change counter, for incremental sync',
    deleted     TINYINT(1)      NOT NULL DEFAULT 0 COMMENT '1 = withdrawn by its owner or an admin',
    created_at  DATETIME        NOT NULL,
    updated_at  DATETIME        NOT NULL,
    PRIMARY KEY (uid),
    KEY idx_rev (rev)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- every create / update / withdraw, so any change can be looked up or undone
CREATE TABLE IF NOT EXISTS plv_history (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    uid         CHAR(36)        NOT NULL,
    rev         BIGINT UNSIGNED NOT NULL,
    action      VARCHAR(16)     NOT NULL COMMENT 'create, update, withdraw, restore',
    by_admin    TINYINT(1)      NOT NULL DEFAULT 0,
    author      VARCHAR(100)    NOT NULL DEFAULT '',
    checksum    CHAR(64)        NOT NULL,
    data        MEDIUMTEXT      NOT NULL,
    ip          VARCHAR(45)     NOT NULL DEFAULT '',
    created_at  DATETIME        NOT NULL,
    PRIMARY KEY (id),
    KEY idx_uid (uid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS plv_counter (
    id  TINYINT UNSIGNED NOT NULL,
    v   BIGINT UNSIGNED  NOT NULL,
    PRIMARY KEY (id)
) ENGINE=InnoDB;
INSERT IGNORE INTO plv_counter (id, v) VALUES (1, 0);

-- simple per-IP request limit (rows older than a few minutes are removed automatically)
CREATE TABLE IF NOT EXISTS plv_rate (
    ip      VARCHAR(45)  NOT NULL,
    kind    CHAR(1)      NOT NULL COMMENT 'r = read, w = write',
    minute  INT UNSIGNED NOT NULL,
    hits    INT UNSIGNED NOT NULL,
    PRIMARY KEY (ip, kind, minute)
) ENGINE=InnoDB;
