-- Ending unlocks survive overwriting the reconnect settlement cache.
CREATE TABLE user_rouge_ending_history (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL,
    ending_id INTEGER NOT NULL CHECK (ending_id BETWEEN 101 AND 104),
    completed_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, season, ending_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Recover only the latest successful ending still available in old caches.
-- CASE protects every JSON read, even when an existing snapshot is malformed.
-- Older endings already overwritten by a later run cannot be reconstructed.
WITH valid_snapshots AS (
    SELECT user_id, season,
           CASE WHEN json_valid(result_info) THEN result_info ELSE '{}' END AS result,
           CASE WHEN json_valid(rouge_info) THEN rouge_info ELSE '{}' END AS info
    FROM user_rouge_settlement
), endings AS (
    SELECT user_id, season,
           COALESCE(json_extract(result, '$.reviewInfo.endId'),
                    json_extract(info, '$.endId')) AS ending_id,
           json_extract(result, '$.end2Score') AS score,
           MAX(0, COALESCE(CAST(json_extract(result, '$.reviewInfo.finishTime') AS INTEGER), 0)) * 1000 AS completed_at
    FROM valid_snapshots
)
INSERT INTO user_rouge_ending_history (user_id, season, ending_id, completed_at)
SELECT user_id, season, CAST(ending_id AS INTEGER), completed_at
FROM endings
WHERE ending_id BETWEEN 101 AND 104
  AND typeof(score) = 'text'
  AND score LIKE CAST(ending_id AS TEXT) || '#%'
  AND length(score) > 4
ON CONFLICT DO NOTHING;
