-- Number of consecutive failures for the current Rouge encounter.
-- The client permits retries while this value is at most rouge_const[43].
ALTER TABLE user_rouge_run ADD COLUMN retry_num INTEGER NOT NULL DEFAULT 0;
