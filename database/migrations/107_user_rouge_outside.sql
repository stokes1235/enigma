-- Meta progression for the original Rouge: 刻画的枝芽 (genius points) and the
-- activated talent-tree nodes.
CREATE TABLE IF NOT EXISTS user_rouge_outside (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    genius_point INTEGER NOT NULL DEFAULT 0,
    point INTEGER NOT NULL DEFAULT 0,
    have_get_point INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, season),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_rouge_genius (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    genius_id INTEGER NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, season, genius_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
