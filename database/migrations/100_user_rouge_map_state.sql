-- Map state for the original Rouge run: normal layer map vs. middle layer (rest area).
ALTER TABLE user_rouge_run ADD COLUMN map_type INTEGER NOT NULL DEFAULT 1;
ALTER TABLE user_rouge_run ADD COLUMN middle_layer_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_rouge_run ADD COLUMN position_index INTEGER NOT NULL DEFAULT 0;
