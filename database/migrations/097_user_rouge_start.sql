-- Original Rouge (A Series of Dusks), separate from Rouge2.
CREATE TABLE user_rouge_start (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    state INTEGER NOT NULL DEFAULT 0,
    difficulty INTEGER NOT NULL DEFAULT 0,
    style INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, season),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
