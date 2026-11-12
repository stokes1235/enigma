-- Run telemetry used by badge scoring.
ALTER TABLE user_rouge_run ADD COLUMN spent_coin INTEGER NOT NULL DEFAULT 0;
