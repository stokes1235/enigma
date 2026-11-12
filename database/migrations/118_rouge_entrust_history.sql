CREATE TABLE user_rouge_entrust_history (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    entrust_id INTEGER NOT NULL,
    PRIMARY KEY (user_id, season, entrust_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
