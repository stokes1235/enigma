-- The selected post-run bonus can increase max stimulus for the next run.
ALTER TABLE user_rouge_run ADD COLUMN power_limit INTEGER NOT NULL DEFAULT 0;
