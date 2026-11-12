-- Score remainders carried between runs ("多出的部分累计到下次演绎").
ALTER TABLE user_rouge_start ADD COLUMN point_remainder INTEGER NOT NULL DEFAULT 0;
ALTER TABLE user_rouge_start ADD COLUMN genius_remainder INTEGER NOT NULL DEFAULT 0;
