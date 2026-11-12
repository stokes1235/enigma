-- Full local run reports are independent of the one-entry reconnect cache.
-- No official retention cap was found: keep every committed run locally.
CREATE TABLE user_rouge_reviews (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL CHECK (season = 1),
    run_seed INTEGER,
    review_info TEXT NOT NULL CHECK (json_valid(review_info)),
    finish_time INTEGER NOT NULL CHECK (finish_time >= 0),
    recorded_at INTEGER NOT NULL CHECK (recorded_at >= 0),
    UNIQUE (user_id, season, run_seed),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
-- The old cache does not contain its run seed. Preserve that uncertainty and
-- import at most one such report per account rather than inventing a run ID.
CREATE UNIQUE INDEX rouge_reviews_one_legacy
    ON user_rouge_reviews (user_id, season) WHERE run_seed IS NULL;
CREATE INDEX rouge_reviews_latest
    ON user_rouge_reviews (user_id, season, finish_time DESC, id DESC);

WITH snapshots AS (
    SELECT user_id, season,
           CASE WHEN json_valid(result_info) THEN result_info ELSE '{}' END AS result
    FROM user_rouge_settlement WHERE season = 1
), reports AS (
    SELECT user_id, season, json_extract(result, '$.reviewInfo') AS review_info,
           MAX(0, COALESCE(CAST(json_extract(result, '$.reviewInfo.finishTime') AS INTEGER), 0)) AS old_time
    FROM snapshots WHERE json_type(result, '$.reviewInfo') = 'object'
)
INSERT INTO user_rouge_reviews
    (user_id, season, run_seed, review_info, finish_time, recorded_at)
SELECT user_id, season, NULL, review_info,
       CASE WHEN old_time > 0 AND old_time < 100000000000 THEN old_time * 1000 ELSE old_time END,
       CASE WHEN old_time > 0 AND old_time < 100000000000 THEN old_time * 1000 ELSE old_time END
FROM reports WHERE 1 ON CONFLICT DO NOTHING;

CREATE TABLE user_rouge_passed_events (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL CHECK (season = 1),
    event_id INTEGER NOT NULL CHECK (event_id > 0),
    first_observed_at INTEGER NOT NULL CHECK (first_observed_at >= 0),
    PRIMARY KEY (user_id, season, event_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE user_rouge_passed_layers (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL CHECK (season = 1),
    layer_id INTEGER NOT NULL CHECK (layer_id > 0),
    PRIMARY KEY (user_id, season, layer_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE user_rouge_illustration_seen (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL CHECK (season = 1),
    illustration_id INTEGER NOT NULL CHECK (illustration_id > 0),
    PRIMARY KEY (user_id, season, illustration_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Observe actual resolved nodes, never all generated nodes or presumed earlier
-- layers. A selected choice still waiting for its reward/recruitment interaction
-- is not a completed current event. All JSON reads tolerate malformed old saves.
CREATE VIEW rouge_review_resolved_events AS
SELECT run.user_id, run.season, MAX(0, run.updated_at) AS first_observed_at,
       CAST(json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.event_id') AS INTEGER) AS event_id
FROM user_rouge_run AS run,
     json_each(CASE WHEN json_valid(run.graph) THEN run.graph ELSE '{}' END,'$.nodes') AS node
WHERE run.season=1
  AND json_type(CASE WHEN json_valid(run.graph) THEN run.graph ELSE '{}' END,'$.nodes')='array'
  AND json_type(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.event_id')='integer'
  AND json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.event_id')>0
  AND json_type(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.node_id')='integer'
  AND json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.node_id')>0
  AND (json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.node_id')<>run.node_id OR run.interactive_index<0)
  AND (
      (json_type(CASE WHEN json_valid(run.finished) THEN run.finished ELSE '[]' END)='array'
       AND EXISTS (
           SELECT 1 FROM json_each(CASE WHEN json_valid(run.finished) THEN run.finished ELSE '[]' END) AS finished
           WHERE finished.type='integer'
             AND finished.value=json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.node_id')
       ))
      OR
      (json_type(CASE WHEN json_valid(run.choices) THEN run.choices ELSE '{}' END)='object'
       AND json_type(CASE WHEN json_valid(run.choices) THEN run.choices ELSE '{}' END,
           '$."' || CAST(json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.node_id') AS TEXT) || '"')='integer'
       AND json_extract(CASE WHEN json_valid(run.choices) THEN run.choices ELSE '{}' END,
           '$."' || CAST(json_extract(CASE WHEN node.type='object' THEN node.value ELSE '{}' END,'$.node_id') AS TEXT) || '"')>0)
  );

CREATE VIEW rouge_review_completed_layers AS
SELECT user_id, season, layer_id FROM user_rouge_run
WHERE season=1 AND layer_id>0 AND (map_type=2 OR terminal_state=1);

INSERT INTO user_rouge_passed_events (user_id,season,event_id,first_observed_at)
SELECT user_id,season,event_id,first_observed_at FROM rouge_review_resolved_events
WHERE 1 ON CONFLICT DO NOTHING;
INSERT INTO user_rouge_passed_layers (user_id,season,layer_id)
SELECT user_id,season,layer_id FROM rouge_review_completed_layers
WHERE 1 ON CONFLICT DO NOTHING;

CREATE TRIGGER rouge_review_run_insert AFTER INSERT ON user_rouge_run
BEGIN
    INSERT INTO user_rouge_passed_events (user_id,season,event_id,first_observed_at)
    SELECT user_id,season,event_id,first_observed_at FROM rouge_review_resolved_events
    WHERE user_id=NEW.user_id AND season=NEW.season ON CONFLICT DO NOTHING;
    INSERT INTO user_rouge_passed_layers (user_id,season,layer_id)
    SELECT user_id,season,layer_id FROM rouge_review_completed_layers
    WHERE user_id=NEW.user_id AND season=NEW.season ON CONFLICT DO NOTHING;
END;
CREATE TRIGGER rouge_review_run_update
AFTER UPDATE OF graph,finished,choices,interactive_index,map_type,terminal_state ON user_rouge_run
BEGIN
    INSERT INTO user_rouge_passed_events (user_id,season,event_id,first_observed_at)
    SELECT user_id,season,event_id,first_observed_at FROM rouge_review_resolved_events
    WHERE user_id=NEW.user_id AND season=NEW.season ON CONFLICT DO NOTHING;
    INSERT INTO user_rouge_passed_layers (user_id,season,layer_id)
    SELECT user_id,season,layer_id FROM rouge_review_completed_layers
    WHERE user_id=NEW.user_id AND season=NEW.season ON CONFLICT DO NOTHING;
END;
CREATE TRIGGER rouge_review_run_delete BEFORE DELETE ON user_rouge_run
BEGIN
    INSERT INTO user_rouge_passed_events (user_id,season,event_id,first_observed_at)
    SELECT user_id,season,event_id,first_observed_at FROM rouge_review_resolved_events
    WHERE user_id=OLD.user_id AND season=OLD.season ON CONFLICT DO NOTHING;
    INSERT INTO user_rouge_passed_layers (user_id,season,layer_id)
    SELECT user_id,season,layer_id FROM rouge_review_completed_layers
    WHERE user_id=OLD.user_id AND season=OLD.season ON CONFLICT DO NOTHING;
END;
