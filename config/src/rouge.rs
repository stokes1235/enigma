//! Handwritten queries for the original Rouge (A Series of Dusks).
//!
//! Everything here is derived from the shipped game data, so the setup flow stays
//! data-driven instead of hardcoding per-style/per-difficulty constants.

use crate::GameDB;
use crate::character::Character;
use crate::rogue_collection::RogueCollection;
use crate::rogue_collection_backpack::RogueCollectionBackpack;
use crate::rouge_choice::RougeChoice;
use crate::rouge_difficulty::RougeDifficulty;
use crate::rouge_enchant::RougeEnchant;
use crate::rouge_event::RougeEvent;
use crate::rouge_fight_event::RougeFightEvent;
use crate::rouge_style::RougeStyle;

/// `rouge_style.unlockType` for catalysts that are available from the start.
const STYLE_UNLOCK_DEFAULT: i32 = 0;
/// `rouge_const` id: noise cost per rarity (`5#4|4#2|3#1|2#1|1#1`).
const CONST_NOISE_BY_RARE: i32 = 2;
/// `rouge_const` id: maximum number of initial companions (8).
const CONST_TEAM_SIZE: i32 = 22;

impl GameDB {
    /// Catalyst (style) definition for a season.
    pub fn rouge_style(&self, season: i32, style: i32) -> Option<&RougeStyle> {
        self.rouge_style
            .iter()
            .find(|row| row.season == season && row.id == style)
    }

    /// Whether a catalyst is available without any meta progression.
    pub fn rouge_style_is_default_unlocked(&self, style: &RougeStyle) -> bool {
        style.unlock_type == STYLE_UNLOCK_DEFAULT
    }

    /// Difficulty (音序) definition.
    pub fn rouge_difficulty(&self, difficulty: i32) -> Option<&RougeDifficulty> {
        self.rouge_difficulty
            .iter()
            .find(|row| row.difficulty == difficulty)
    }

    /// Exact exported layer-difficulty parameters. These are not a complete
    /// monster scaling formula: the server-side use of `stepAttr` is missing.
    pub fn rouge_layer_step_attr(&self, difficulty: i32, layer: i32) -> Option<&str> {
        self.rouge_layer_difficulty
            .iter()
            .find(|row| row.difficulty == difficulty && row.layer == layer)
            .map(|row| row.step_attr.as_str())
    }

    /// Hidden initial collections identify cumulative noise rules; they are
    /// not player-owned bag items. Levels5/10/15 have additional numeric rules
    /// described by the difficulty row without adding a collection ID.
    pub fn rouge_difficulty_initial_collections(&self, difficulty: i32) -> Option<Vec<i32>> {
        self.rouge_difficulty(difficulty)?
            .init_collections
            .split('#')
            .filter(|value| !value.is_empty())
            .map(|value| value.parse::<i32>().ok().filter(|id| *id > 0))
            .collect()
    }

    /// Official unlock chain: `preDifficulty == 0` is always available,
    /// otherwise the previous difficulty must have been cleared.
    pub fn rouge_difficulty_unlocked(&self, difficulty: i32, cleared: &[i32]) -> bool {
        match self.rouge_difficulty(difficulty) {
            Some(row) => row.pre_difficulty == 0 || cleared.contains(&row.pre_difficulty),
            None => false,
        }
    }

    /// Maximum number of companions selected before a run starts.
    pub fn rouge_team_size(&self) -> usize {
        self.rouge_const
            .get(CONST_TEAM_SIZE)
            .and_then(|row| row.value.trim().parse::<usize>().ok())
            .unwrap_or(8)
    }

    /// Noise cost for one character, keyed by `character.rare` (1..=5).
    ///
    /// Official wording uses star ratings; the data stores `rare` one lower
    /// (6★ -> 5, 5★ -> 4, <=4★ -> <=3).
    pub fn rouge_noise_cost(&self, rare: i32) -> Option<i32> {
        let raw = self.rouge_const.get(CONST_NOISE_BY_RARE)?.value.clone();
        raw.split('|').find_map(|entry| {
            let (key, value) = entry.split_once('#')?;
            (key.trim().parse::<i32>().ok()? == rare)
                .then(|| value.trim().parse::<i32>().ok())
                .flatten()
        })
    }

    /// Event row for a node (`rouge_event`).
    pub fn rouge_event(&self, event_id: i32) -> Option<&RougeEvent> {
        self.rouge_event.iter().find(|row| row.id == event_id)
    }

    /// Artefact definition (note the `rogue_` prefix used by the shipped table).
    pub fn rouge_collection(&self, collection_id: i32) -> Option<&RogueCollection> {
        self.rogue_collection.get(collection_id)
    }

    /// Enchant that can be inlaid into an artefact hole.
    pub fn rouge_enchant(&self, enchant_id: i32) -> Option<&RougeEnchant> {
        self.rouge_enchant.get(enchant_id)
    }

    /// Number of inlay holes an artefact exposes (`holeNum`).
    pub fn rouge_collection_holes(&self, collection_id: i32) -> Option<i32> {
        self.rouge_collection(collection_id).map(|row| row.hole_num)
    }

    /// Backpack layout used by a catalyst (`rouge_style.layoutId`).
    ///
    /// The shipped table is spelled `rogue_collection_backpack` (historical typo)
    /// and is the only `rogue_` table this mode reads.
    pub fn rouge_backpack(&self, layout_id: i32) -> Option<&RogueCollectionBackpack> {
        self.rogue_collection_backpack.get(layout_id)
    }

    /// Fight definition for an event, when the node is a battle.
    pub fn rouge_fight_event(&self, event_id: i32) -> Option<&RougeFightEvent> {
        self.rouge_fight_event.iter().find(|row| row.id == event_id)
    }

    /// Episode a fight event resolves to (0 when the row has no episode).
    pub fn rouge_fight_episode(&self, event_id: i32) -> Option<i32> {
        self.rouge_fight_event(event_id)
            .map(|row| row.episode_id)
            .filter(|episode| *episode != 0)
    }

    /// Choices belonging to an event.
    ///
    /// `rouge_choice` has no `eventId` column: the event id is encoded in the
    /// choice id (`10510101 -> 105101`), which matches 529/533 shipped rows.
    pub fn rouge_choices_for_event(&self, event_id: i32) -> Vec<&RougeChoice> {
        // Nested story phases use explicit menu IDs: the second phase has an
        // extra digit, and cannot be grouped by choice.id / 100.
        let phase: Option<&[i32]> = match event_id {
            106113 => Some(&[10611301, 10611302, 10611303]),
            1061131 => Some(&[10611311, 10611312, 10611313]),
            1061132 => Some(&[10611321, 10611322, 10611323]),
            106141 => Some(&[10614101, 10614102]),
            106151 => Some(&[10615101, 10615102]),
            1061511 => Some(&[10615111, 10615112]),
            106152 => Some(&[10615201, 10615202]),
            1061521 => Some(&[10615211, 10615212]),
            106153 => Some(&[10615301, 10615302]),
            _ => None,
        };
        if let Some(ids) = phase {
            return ids
                .iter()
                .filter_map(|id| self.rouge_choice.get(*id))
                .collect();
        }
        // Treasure and respite encounters share their menu across many event
        // ids. The 3.9 trace includes e.g. 105030 -> 10510101 and
        // 108014 -> 10810105; their ids do not encode an individual menu.
        let menu = match self.rouge_choice_event.get(event_id).map(|row| row.r#type) {
            Some(5) => 105101,
            Some(8) => 108101,
            _ => event_id,
        };
        let mut choices: Vec<&RougeChoice> = self
            .rouge_choice
            .iter()
            .filter(|row| row.id / 100 == menu)
            .collect();
        choices.sort_by_key(|row| row.id);
        choices
    }

    /// Character definition used for rarity lookup.
    pub fn rouge_character(&self, hero_id: i32) -> Option<&Character> {
        self.character.get(hero_id)
    }
}
