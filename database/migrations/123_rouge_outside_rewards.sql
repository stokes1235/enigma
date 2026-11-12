-- Original Rouge's permanent reward board, separate from next-run rewards.
CREATE TABLE user_rouge_reward_claims (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    stage INTEGER NOT NULL CHECK (stage > 0),
    bonus_id INTEGER NOT NULL CHECK (bonus_id > 0),
    claimed_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, season, bonus_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE user_rouge_reward_meta (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    seen_stage INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, season),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
