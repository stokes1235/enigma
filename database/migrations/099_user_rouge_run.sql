-- Active exploration run for the original Rouge (A Series of Dusks).
-- One run per user and season; the generated layer graph is stored as JSON so the
-- map stays identical across reconnects and requests.
CREATE TABLE IF NOT EXISTS user_rouge_run (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    layer_index INTEGER NOT NULL DEFAULT 0,
    layer_id INTEGER NOT NULL DEFAULT 0,
    stage INTEGER NOT NULL DEFAULT 0,
    node_id INTEGER NOT NULL DEFAULT 0,
    seed INTEGER NOT NULL DEFAULT 0,
    graph TEXT NOT NULL DEFAULT '{}',
    coin INTEGER NOT NULL DEFAULT 0,
    power INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, season),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
