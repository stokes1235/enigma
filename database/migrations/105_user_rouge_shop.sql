-- Current shop stock for the active run, stored as JSON.
ALTER TABLE user_rouge_run ADD COLUMN shop TEXT NOT NULL DEFAULT '';
