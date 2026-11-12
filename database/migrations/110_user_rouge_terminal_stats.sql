-- Terminal outcome and counters carried across generated layer graphs.
ALTER TABLE user_rouge_run ADD COLUMN terminal_state INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_rouge_run ADD COLUMN minor_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_rouge_run ADD COLUMN danger_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_rouge_run ADD COLUMN crisis_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_rouge_run ADD COLUMN step_count INTEGER NOT NULL DEFAULT 0;
