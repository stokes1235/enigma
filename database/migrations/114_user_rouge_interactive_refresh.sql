-- Number of rerolls used by the current collection-drop interaction.
ALTER TABLE user_rouge_run ADD COLUMN interactive_refresh INTEGER NOT NULL DEFAULT 0;
