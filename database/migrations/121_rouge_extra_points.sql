-- Craftmaster's Favors are a separate, persistent weekly bonus balance.
-- NULL means this local account has not received its first weekly login grant.
-- Existing Rouge/account progression is intentionally retained.
CREATE TABLE user_rouge_extra_points (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    balance INTEGER NOT NULL DEFAULT 0 CHECK (balance >= 0),
    recovery_week INTEGER,
    PRIMARY KEY (user_id, season),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
