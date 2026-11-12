-- Original Rouge (A Series of Dusks) setup: hero selection + difficulty progress.
ALTER TABLE user_rouge_start ADD COLUMN hero_ids TEXT NOT NULL DEFAULT '[]';
ALTER TABLE user_rouge_start ADD COLUMN assist_hero_uid INTEGER NOT NULL DEFAULT 0;

-- Cleared difficulties drive the official unlock chain
-- (clear 1 -> unlock 2..5, clear 5 -> unlock 6..10, then sequential).
CREATE TABLE IF NOT EXISTS user_rouge_progress (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    difficulty INTEGER NOT NULL,
    cleared INTEGER NOT NULL DEFAULT 0,
    best_score INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, season, difficulty),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
