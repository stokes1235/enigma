-- Chosen event options per node, stored as a JSON object { "node_id": choice_id }.
ALTER TABLE user_rouge_run ADD COLUMN choices TEXT NOT NULL DEFAULT '{}';
