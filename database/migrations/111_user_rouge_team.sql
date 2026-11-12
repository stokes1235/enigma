-- Persist the current four-slot combat party and every recruited hero's life.
ALTER TABLE user_rouge_run ADD COLUMN team TEXT NOT NULL DEFAULT '{}';
