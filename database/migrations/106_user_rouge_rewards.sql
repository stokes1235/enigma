-- Rewards offered after settlement, claimed through EnterRougeSelectReward.
ALTER TABLE user_rouge_start ADD COLUMN rewards TEXT NOT NULL DEFAULT '[]';
