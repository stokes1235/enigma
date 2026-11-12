-- Backpack placement for the active run, stored as JSON.
ALTER TABLE user_rouge_run ADD COLUMN bag TEXT NOT NULL DEFAULT '';
