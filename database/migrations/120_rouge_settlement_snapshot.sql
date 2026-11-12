-- Replay the same settlement after reconnect without crediting rewards twice.
CREATE TABLE user_rouge_settlement (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    rouge_info TEXT NOT NULL,
    result_info TEXT NOT NULL,
    PRIMARY KEY (user_id, season),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
