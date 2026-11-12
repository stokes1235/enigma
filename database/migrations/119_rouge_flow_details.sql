-- Run-local talent choices, crafting history and ending battle authorization.
ALTER TABLE user_rouge_run ADD COLUMN details TEXT NOT NULL DEFAULT '{}';
