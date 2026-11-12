-- Previous builds persisted Rouge hero life on a 0-100000 scale. The client
-- and captured server use per-mille values (0-1000).
UPDATE user_rouge_run
SET team = json_set(
    team,
    '$.heroLifeList',
    json((SELECT json_group_array(json_set(value, '$.life',
                  CAST(COALESCE(json_extract(value, '$.life'), 0) / 100 AS INTEGER)))
          FROM json_each(user_rouge_run.team, '$.heroLifeList')))
)
WHERE json_valid(team) AND json_type(team, '$.heroLifeList') = 'array';
