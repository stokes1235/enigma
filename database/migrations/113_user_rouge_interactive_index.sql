-- Index of the current interaction in the selected choice's `interactive`
-- pipeline. -1 means the choice has no server-supported interaction pending.
ALTER TABLE user_rouge_run ADD COLUMN interactive_index INTEGER NOT NULL DEFAULT -1;
