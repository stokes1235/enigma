-- Original Rouge: drop-pool availability is NOT acquisition history.
-- Freeze the verified season-1, no-version editor catalog for DB triggers.
CREATE TABLE rouge_favorite_base_catalog (
    collection_id INTEGER PRIMARY KEY
);
INSERT INTO rouge_favorite_base_catalog (collection_id) VALUES
(8110021),(8110029),(8110030),(8110048),(8110213),(8110307),(8110312),(8110410),(8110507),(8110616),
(8110716),(8110805),(8120034),(8120035),(8120047),(8120104),(8120110),(8120208),(8120216),(8120317),
(8120406),(8120416),(8120512),(8120610),(8120703),(8120704),(8120705),(8120808),(8120821),(8130018),
(8130041),(8130103),(8130107),(8130115),(8130203),(8130217),(8130303),(8130315),(8130403),(8130404),
(8130503),(8130504),(8130505),(8130603),(8130604),(8130605),(8130803),(8130804),(8140101),(8140201),
(8140301),(8140401),(8140501),(8140601),(8140701),(8140801),(8150006),(8150008),(8150010),(8150012),
(8150014),(8150016),(8180501),(8180502),(8180503),(8210031),(8210032),(8210045),(8210105),(8210113),
(8210205),(8210209),(8210308),(8210313),(8210314),(8210411),(8210508),(8210611),(8210711),(8210712),
(8210713),(8210818),(8210819),(8220033),(8220036),(8220044),(8220051),(8220052),(8220106),(8220214),
(8220215),(8220318),(8220409),(8220415),(8220515),(8220516),(8220612),(8220613),(8220806),(8220807),
(8220813),(8220814),(8220820),(8230004),(8230019),(8230024),(8230027),(8230028),(8230037),(8230038),
(8230039),(8230040),(8230042),(8230050),(8230204),(8230304),(8230305),(8230316),(8230414),(8230509),
(8230606),(8230614),(8230714),(8240102),(8240202),(8240302),(8240402),(8240502),(8240602),(8240702),
(8240802),(8250007),(8250009),(8250011),(8250013),(8250015),(8250017),(8310022),(8310108),(8310114),
(8310206),(8310212),(8310309),(8310315),(8310405),(8310407),(8310510),(8310608),(8310609),(8310706),
(8310810),(8310815),(8320025),(8320046),(8320049),(8320109),(8320207),(8320210),(8320310),(8320320),
(8320408),(8320506),(8320511),(8320514),(8320617),(8320618),(8320707),(8320709),(8320809),(8320811),
(8320812),(8320907),(8330001),(8330002),(8330003),(8330005),(8330020),(8330023),(8330026),(8330111),
(8330112),(8330211),(8330218),(8330306),(8330311),(8330412),(8330413),(8330417),(8330418),(8330513),
(8330607),(8330615),(8330708),(8330710),(8330715),(8330816),(8330817),(8350043);

CREATE TABLE user_rouge_favorite_collections (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL CHECK (season = 1),
    collection_id INTEGER NOT NULL,
    first_obtained_at INTEGER NOT NULL CHECK (first_obtained_at >= 0),
    seen INTEGER NOT NULL DEFAULT 0 CHECK (seen IN (0,1)),
    PRIMARY KEY (user_id, season, collection_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (collection_id) REFERENCES rouge_favorite_base_catalog(collection_id)
);
CREATE TABLE user_rouge_favorite_stories (
    user_id INTEGER NOT NULL,
    season INTEGER NOT NULL CHECK (season = 1),
    story_id INTEGER NOT NULL,
    review_id INTEGER NOT NULL,
    unlocked_at INTEGER NOT NULL CHECK (unlocked_at >= 0),
    seen INTEGER NOT NULL DEFAULT 0 CHECK (seen IN (0,1)),
    PRIMARY KEY (user_id, season, story_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Keep extraction in one view, shared by migration/backfill and both triggers.
-- Guard malformed legacy JSON and primitive items before json_extract. Only
-- integer config IDs in actual item_id/holes fields are acquisition evidence;
-- a UID, seed, rotation or a string mentioning an ID is not evidence.
CREATE VIEW rouge_favorite_current_bag_ids AS
SELECT run.user_id, run.season, MAX(0,run.updated_at) AS obtained_at,
       CAST(json_extract(CASE WHEN item.type='object' THEN item.value ELSE '{}' END,'$.item_id') AS INTEGER) AS collection_id
FROM user_rouge_run AS run,
     json_each(CASE WHEN json_valid(run.bag) THEN run.bag ELSE '{}' END,'$.items') AS item
WHERE run.season=1
  AND json_type(CASE WHEN json_valid(run.bag) THEN run.bag ELSE '{}' END,'$.items')='array'
  AND json_type(CASE WHEN item.type='object' THEN item.value ELSE '{}' END,'$.item_id')='integer'
UNION
SELECT run.user_id, run.season, MAX(0,run.updated_at), CAST(hole.value AS INTEGER)
FROM user_rouge_run AS run,
     json_each(CASE WHEN json_valid(run.bag) THEN run.bag ELSE '{}' END,'$.items') AS item,
     json_each(CASE WHEN item.type='object' THEN item.value ELSE '{}' END,'$.holes') AS hole
WHERE run.season=1 AND hole.type='integer'
  AND json_type(CASE WHEN json_valid(run.bag) THEN run.bag ELSE '{}' END,'$.items')='array'
  AND json_type(CASE WHEN item.type='object' THEN item.value ELSE '{}' END,'$.holes')='array';

-- Recover only items actually present in an old current bag. Lost historical
-- items and unsupported review formats cannot be truthfully reconstructed.
INSERT INTO user_rouge_favorite_collections
    (user_id,season,collection_id,first_obtained_at)
SELECT owned.user_id,owned.season,owned.collection_id,owned.obtained_at
FROM rouge_favorite_current_bag_ids AS owned
JOIN rouge_favorite_base_catalog AS catalog USING (collection_id)
WHERE 1 ON CONFLICT DO NOTHING;

-- Triggers execute in the owning action/settlement transaction. They cover
-- initial bags, direct battle rewards, purchases, forging and legacy writers.
-- Repeated placements/inlays do not reset seen flags or first acquisition time.
CREATE TRIGGER rouge_favorite_bag_insert AFTER INSERT ON user_rouge_run
BEGIN
    INSERT INTO user_rouge_favorite_collections
        (user_id,season,collection_id,first_obtained_at)
    SELECT owned.user_id,owned.season,owned.collection_id,owned.obtained_at
    FROM rouge_favorite_current_bag_ids AS owned
    JOIN rouge_favorite_base_catalog AS catalog USING (collection_id)
    WHERE owned.user_id=NEW.user_id AND owned.season=NEW.season
    ON CONFLICT DO NOTHING;
END;
CREATE TRIGGER rouge_favorite_bag_update AFTER UPDATE OF bag ON user_rouge_run
BEGIN
    INSERT INTO user_rouge_favorite_collections
        (user_id,season,collection_id,first_obtained_at)
    SELECT owned.user_id,owned.season,owned.collection_id,owned.obtained_at
    FROM rouge_favorite_current_bag_ids AS owned
    JOIN rouge_favorite_base_catalog AS catalog USING (collection_id)
    WHERE owned.user_id=NEW.user_id AND owned.season=NEW.season
    ON CONFLICT DO NOTHING;
END;
