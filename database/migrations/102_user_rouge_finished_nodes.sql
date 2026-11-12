-- Node ids already resolved (fight won / event taken), stored as a JSON array.
ALTER TABLE user_rouge_run ADD COLUMN finished TEXT NOT NULL DEFAULT '[]';
