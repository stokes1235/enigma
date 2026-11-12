//! Backpack (造物布设) for the original Rouge.
//!
//! Each catalyst points at a `rogue_collection_backpack` row (note the historical
//! `rogue_` spelling in the shipped data):
//! - `layout`            `cols#rows` of the placement grid (`8#8`)
//! - `initialCollection` `collection_id#rotation#row#col`
//!
//! Only ids and positions are authoritative server-side: the client resolves the
//! artefact's shape and effects from its own tables.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sonettobuf::{RougeBag, RougeBagPos, RougeItem, RougeItemLayout, RougeWarehouse};
use std::collections::HashSet;

use super::collection_shapes;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BagItem {
    pub uid: i64,
    pub item_id: i32,
    pub row: i32,
    pub col: i32,
    #[serde(default)]
    pub rotation: i32,
    /// Inlaid enchant config ids, one slot per artefact hole (0 = empty).
    #[serde(default)]
    pub holes: Vec<i32>,
    /// Instance UIDs parallel to `holes`.
    #[serde(default)]
    pub hole_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunBag {
    pub rows: i32,
    pub cols: i32,
    pub items: Vec<BagItem>,
    #[serde(default)]
    pub uid_counter: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayChange {
    pub target: BagItem,
    pub previous_holder: Option<BagItem>,
    pub removed_from_warehouse: bool,
    pub removed_from_bag: bool,
    pub returned_to_warehouse: Option<BagItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemountChange {
    pub target: BagItem,
    pub enchant: BagItem,
}

impl RunBag {
    /// Parses the shipped `layout` / `initialCollection` pair.
    pub fn from_layout(layout: &str, initial: &str) -> Result<Self, AppError> {
        let (cols, rows) = layout.split_once('#').ok_or(AppError::InvalidRequest)?;
        let cols: i32 = cols.trim().parse().map_err(|_| AppError::InvalidRequest)?;
        let rows: i32 = rows.trim().parse().map_err(|_| AppError::InvalidRequest)?;
        if rows <= 0 || cols <= 0 {
            return Err(AppError::InvalidRequest);
        }

        let mut bag = Self {
            rows,
            cols,
            items: Vec::new(),
            uid_counter: 0,
        };
        for entry in initial.split('|').filter(|entry| !entry.trim().is_empty()) {
            let mut fields = entry.split('#');
            let item_id: i32 = fields
                .next()
                .and_then(|value| value.trim().parse().ok())
                .ok_or(AppError::InvalidRequest)?;
            let rotation: i32 = fields
                .next()
                .and_then(|value| value.trim().parse().ok())
                .ok_or(AppError::InvalidRequest)?;
            let row: i32 = fields
                .next()
                .and_then(|value| value.trim().parse().ok())
                .ok_or(AppError::InvalidRequest)?;
            let col: i32 = fields
                .next()
                .and_then(|value| value.trim().parse().ok())
                .ok_or(AppError::InvalidRequest)?;
            if fields.next().is_some() || item_id == 0 || !(0..=3).contains(&rotation) {
                return Err(AppError::InvalidRequest);
            }
            let item = BagItem {
                uid: bag.items.len() as i64 + 1,
                item_id,
                row,
                col,
                rotation,
                holes: Vec::new(),
                hole_ids: Vec::new(),
            };
            if !bag.can_place(&item, row, col, rotation)? {
                return Err(AppError::InvalidRequest);
            }
            bag.items.push(item);
        }

        Ok(bag)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Adds an artefact to the warehouse. Placement is a separate operation.
    pub fn add(&mut self, item_id: i32) -> Result<BagItem, AppError> {
        if self.rows <= 0 || self.cols <= 0 {
            return Err(AppError::InvalidRequest);
        }
        let item = BagItem {
            uid: self.next_uid(),
            item_id,
            row: -1,
            col: -1,
            rotation: 0,
            holes: vec![0; collection_shapes::hole_num(item_id).max(0) as usize],
            hole_ids: vec![0; collection_shapes::hole_num(item_id).max(0) as usize],
        };
        self.items.push(item.clone());
        self.uid_counter = item.uid;
        Ok(item)
    }

    pub fn item(&self, uid: i64) -> Option<&BagItem> {
        self.items.iter().find(|item| item.uid == uid)
    }

    pub fn is_placed(item: &BagItem) -> bool {
        item.row >= 0 && item.col >= 0
    }

    /// Places or moves an artefact. `(-1, -1)` requests the first free position.
    /// The returned bool is true when the item came from the warehouse.
    pub fn place(
        &mut self,
        uid: i64,
        row: i32,
        col: i32,
        rotation: i32,
    ) -> Result<(BagItem, bool), AppError> {
        if !(0..=3).contains(&rotation) || (row == -1) != (col == -1) {
            return Err(AppError::InvalidRequest);
        }
        let current = self.item(uid).cloned().ok_or(AppError::InvalidRequest)?;
        collection_shapes::get(current.item_id).ok_or(AppError::InvalidRequest)?;
        let was_in_warehouse = !Self::is_placed(&current);
        let (row, col) = if row == -1 {
            self.first_free_position(&current, rotation)?
        } else {
            if !self.can_place(&current, row, col, rotation)? {
                return Err(AppError::InvalidRequest);
            }
            (row, col)
        };

        let item = self
            .items
            .iter_mut()
            .find(|item| item.uid == uid)
            .ok_or(AppError::InvalidRequest)?;
        item.row = row;
        item.col = col;
        item.rotation = rotation;
        Ok((item.clone(), was_in_warehouse))
    }

    /// Auto-places every warehouse artefact that can fit, trying all rotations.
    pub fn place_all(&mut self) -> Vec<BagItem> {
        let ids: Vec<i64> = self
            .items
            .iter()
            .filter(|item| !Self::is_placed(item))
            .map(|item| item.uid)
            .collect();
        let mut placed = Vec::new();
        for uid in ids {
            for rotation in 0..=3 {
                if let Ok((item, _)) = self.place(uid, -1, -1, rotation) {
                    placed.push(item);
                    break;
                }
            }
        }
        placed
    }

    /// Moves one removable artefact back to the warehouse.
    pub fn remove(&mut self, uid: i64) -> Result<BagItem, AppError> {
        let item = self.item(uid).cloned().ok_or(AppError::InvalidRequest)?;
        if !Self::is_placed(&item)
            || collection_shapes::get(item.item_id).is_some_and(|shape| shape.unremovable)
        {
            return Err(AppError::InvalidRequest);
        }
        let item = self
            .items
            .iter_mut()
            .find(|item| item.uid == uid)
            .ok_or(AppError::InvalidRequest)?;
        item.row = -1;
        item.col = -1;
        item.rotation = 0;
        Ok(item.clone())
    }

    /// Clears the board. The client only sends this request when no fixed item
    /// is present, so reject instead of returning a misleading partial result.
    pub fn remove_all(&mut self) -> Result<Vec<BagItem>, AppError> {
        if self
            .items
            .iter()
            .filter(|item| Self::is_placed(item))
            .any(|item| collection_shapes::get(item.item_id).is_some_and(|shape| shape.unremovable))
        {
            return Err(AppError::InvalidRequest);
        }
        let ids: Vec<i64> = self
            .items
            .iter()
            .filter(|item| Self::is_placed(item))
            .map(|item| item.uid)
            .collect();
        ids.into_iter().map(|uid| self.remove(uid)).collect()
    }

    fn first_free_position(&self, item: &BagItem, rotation: i32) -> Result<(i32, i32), AppError> {
        for row in 0..self.rows {
            for col in 0..self.cols {
                if self.can_place(item, row, col, rotation)? {
                    return Ok((row, col));
                }
            }
        }
        Err(AppError::InvalidRequest)
    }

    fn can_place(
        &self,
        item: &BagItem,
        row: i32,
        col: i32,
        rotation: i32,
    ) -> Result<bool, AppError> {
        let cells =
            collection_shapes::cells(item.item_id, rotation).ok_or(AppError::InvalidRequest)?;
        let target: HashSet<(i32, i32)> = cells
            .into_iter()
            .map(|(cell_row, cell_col)| (row + cell_row, col + cell_col))
            .collect();
        if target.is_empty()
            || target.iter().any(|(cell_row, cell_col)| {
                !(0..self.rows).contains(cell_row) || !(0..self.cols).contains(cell_col)
            })
        {
            return Ok(false);
        }

        for other in self
            .items
            .iter()
            .filter(|other| other.uid != item.uid && Self::is_placed(other))
        {
            let other_cells = collection_shapes::cells(other.item_id, other.rotation)
                .ok_or(AppError::InvalidRequest)?;
            if other_cells.into_iter().any(|(cell_row, cell_col)| {
                target.contains(&(other.row + cell_row, other.col + cell_col))
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Moves one enchant instance from the warehouse, the placement grid or
    /// another holder into a target hole. A displaced enchant returns to the warehouse.
    pub fn inlay(
        &mut self,
        target_uid: i64,
        hole: i32,
        enchant_uid: i64,
        hole_num: i32,
    ) -> Result<InlayChange, AppError> {
        if hole < 0 || hole >= hole_num || enchant_uid <= 0 || enchant_uid == target_uid {
            return Err(AppError::InvalidRequest);
        }

        let target = self
            .items
            .iter()
            .find(|item| item.uid == target_uid)
            .ok_or(AppError::InvalidRequest)?;
        if collection_shapes::is_enchant_item(target.item_id) {
            return Err(AppError::InvalidRequest);
        }

        let mut removed_from_warehouse = false;
        let mut removed_from_bag = false;
        let mut previous_holder = None;
        let enchant_item_id;
        if let Some(index) = self.items.iter().position(|item| item.uid == enchant_uid) {
            let enchant = &self.items[index];
            if !collection_shapes::is_enchant_item(enchant.item_id) {
                return Err(AppError::InvalidRequest);
            }
            enchant_item_id = enchant.item_id;
            removed_from_bag = Self::is_placed(enchant);
            removed_from_warehouse = !removed_from_bag;
            self.items.remove(index);
        } else {
            let mut source = None;
            for (item_index, item) in self.items.iter().enumerate() {
                if let Some(slot) = item.hole_ids.iter().position(|uid| *uid == enchant_uid) {
                    source = Some((item_index, slot));
                    break;
                }
            }
            let (source_index, source_slot) = source.ok_or(AppError::InvalidRequest)?;
            enchant_item_id = *self.items[source_index]
                .holes
                .get(source_slot)
                .ok_or(AppError::InvalidRequest)?;
            if !collection_shapes::is_enchant_item(enchant_item_id) {
                return Err(AppError::InvalidRequest);
            }
            self.items[source_index].holes[source_slot] = 0;
            self.items[source_index].hole_ids[source_slot] = 0;
            if self.items[source_index].uid != target_uid {
                previous_holder = Some(self.items[source_index].clone());
            }
        }

        let target_index = self
            .items
            .iter()
            .position(|item| item.uid == target_uid)
            .ok_or(AppError::InvalidRequest)?;
        self.items[target_index].holes.resize(hole_num as usize, 0);
        self.items[target_index]
            .hole_ids
            .resize(hole_num as usize, 0);

        let slot = hole as usize;
        let displaced_item_id = self.items[target_index].holes[slot];
        let displaced_uid = self.items[target_index].hole_ids[slot];
        let returned_to_warehouse = if displaced_item_id != 0 {
            if !collection_shapes::is_enchant_item(displaced_item_id) {
                return Err(AppError::InvalidRequest);
            }
            Some(BagItem {
                uid: if displaced_uid > 0 {
                    displaced_uid
                } else {
                    self.next_uid()
                },
                item_id: displaced_item_id,
                row: -1,
                col: -1,
                rotation: 0,
                holes: Vec::new(),
                hole_ids: Vec::new(),
            })
        } else {
            None
        };
        self.items[target_index].holes[slot] = enchant_item_id;
        self.items[target_index].hole_ids[slot] = enchant_uid;
        let target = self.items[target_index].clone();
        if let Some(item) = returned_to_warehouse.as_ref() {
            self.items.push(item.clone());
        }

        Ok(InlayChange {
            target,
            previous_holder,
            removed_from_warehouse,
            removed_from_bag,
            returned_to_warehouse,
        })
    }

    /// Removes an enchant from a hole and restores the same instance to the warehouse.
    pub fn demount(
        &mut self,
        target_uid: i64,
        hole: i32,
        hole_num: i32,
    ) -> Result<DemountChange, AppError> {
        if hole < 0 || hole >= hole_num {
            return Err(AppError::InvalidRequest);
        }
        let next_uid = self.next_uid();
        let item = self
            .items
            .iter_mut()
            .find(|item| item.uid == target_uid)
            .ok_or(AppError::InvalidRequest)?;
        item.holes.resize(hole_num as usize, 0);
        item.hole_ids.resize(hole_num as usize, 0);
        let slot = hole as usize;
        let enchant_item_id = item.holes[slot];
        let enchant_uid = item.hole_ids[slot];
        if !collection_shapes::is_enchant_item(enchant_item_id) {
            return Err(AppError::InvalidRequest);
        }
        item.holes[slot] = 0;
        item.hole_ids[slot] = 0;
        let target = item.clone();
        let enchant = BagItem {
            uid: if enchant_uid > 0 {
                enchant_uid
            } else {
                next_uid
            },
            item_id: enchant_item_id,
            row: -1,
            col: -1,
            rotation: 0,
            holes: Vec::new(),
            hole_ids: Vec::new(),
        };
        self.items.push(enchant.clone());
        Ok(DemountChange { target, enchant })
    }

    fn next_uid(&self) -> i64 {
        self.items
            .iter()
            .flat_map(|item| std::iter::once(item.uid).chain(item.hole_ids.iter().copied()))
            .max()
            .unwrap_or(0)
            .max(self.uid_counter)
            + 1
    }

    /// Client-facing payload for one placed artefact.
    pub fn to_client_item(item: &BagItem) -> RougeItem {
        RougeItem {
            id: Some(item.uid),
            item_id: Some(item.item_id),
            hold_items: item.holes.clone(),
            hold_ids: item.hole_ids.clone(),
            ..Default::default()
        }
    }

    pub fn to_client_layout(item: &BagItem) -> RougeItemLayout {
        RougeItemLayout {
            pos: Some(RougeBagPos {
                row: Some(item.row),
                col: Some(item.col),
            }),
            rotation: Some(item.rotation),
            item: Some(Self::to_client_item(item)),
            ..Default::default()
        }
    }

    /// Client-facing placement list.
    pub fn to_client(&self) -> RougeBag {
        RougeBag {
            layouts: self
                .items
                .iter()
                .filter(|item| Self::is_placed(item))
                .map(Self::to_client_layout)
                .collect(),
        }
    }

    pub fn to_warehouse(&self) -> RougeWarehouse {
        RougeWarehouse {
            items: self
                .items
                .iter()
                .filter(|item| !Self::is_placed(item))
                .map(Self::to_client_item)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shipped_layout_and_initial_artefact() {
        let bag = RunBag::from_layout("8#8", "8230024#0#3#3").unwrap();
        assert_eq!((bag.rows, bag.cols), (8, 8));
        assert_eq!(bag.items.len(), 1);
        assert_eq!(bag.items[0].item_id, 8230024);
        assert_eq!((bag.items[0].row, bag.items[0].col), (3, 3));
        assert_eq!(bag.items[0].rotation, 0);
        assert_eq!(bag.items[0].uid, 1);
    }

    #[test]
    fn rejects_malformed_layouts_and_out_of_grid_positions() {
        assert!(RunBag::from_layout("", "").is_err());
        assert!(RunBag::from_layout("8x8", "").is_err());
        assert!(RunBag::from_layout("0#8", "").is_err());
        assert!(RunBag::from_layout("8#8", "8230024#9#3#3").is_err());
        assert!(RunBag::from_layout("8#8", "0#1#1#1").is_err());
        assert!(RunBag::from_layout("8#8", "abc#1#1").is_err());
        assert!(RunBag::from_layout("2#2", "8230024#0#1#1").is_err());
    }

    #[test]
    fn client_payload_mirrors_the_placement() {
        let bag = RunBag::from_layout("8#8", "8230024#0#3#3").unwrap();
        let payload = bag.to_client();
        assert_eq!(payload.layouts.len(), 1);
        let layout = &payload.layouts[0];
        assert_eq!(layout.pos.as_ref().unwrap().row, Some(3));
        assert_eq!(layout.pos.as_ref().unwrap().col, Some(3));
        assert_eq!(layout.item.as_ref().unwrap().item_id, Some(8230024));
    }

    #[test]
    fn acquired_items_start_in_the_warehouse() {
        let mut bag = RunBag::from_layout("8#8", "8230024#0#3#3").unwrap();
        let acquired = bag.add(8120512).unwrap();
        assert_eq!((acquired.row, acquired.col), (-1, -1));
        assert_eq!(bag.to_client().layouts.len(), 1);
        assert_eq!(bag.to_warehouse().items.len(), 1);
    }

    #[test]
    fn placement_checks_shape_overlap_rotation_and_bounds() {
        let mut bag = RunBag::from_layout("8#8", "8230024#0#3#3").unwrap();
        let acquired = bag.add(8120512).unwrap();
        assert!(bag.place(acquired.uid, 4, 4, 0).is_err());
        let (placed, from_warehouse) = bag.place(acquired.uid, 0, 6, 1).unwrap();
        assert!(from_warehouse);
        assert_eq!((placed.row, placed.col, placed.rotation), (0, 6, 1));
        assert!(bag.place(acquired.uid, 7, 7, 1).is_err());
    }

    #[test]
    fn automatic_placement_and_removal_round_trip() {
        let mut bag = RunBag::from_layout("3#3", "").unwrap();
        let acquired = bag.add(8120512).unwrap();
        let (placed, from_warehouse) = bag.place(acquired.uid, -1, -1, 0).unwrap();
        assert!(from_warehouse);
        assert_eq!((placed.row, placed.col), (0, 0));
        let removed = bag.remove(acquired.uid).unwrap();
        assert!(!RunBag::is_placed(&removed));
    }

    #[test]
    fn fixed_collections_cannot_be_removed() {
        let mut bag = RunBag::from_layout("8#8", "8220619#0#0#0").unwrap();
        assert!(bag.remove(1).is_err());
        assert!(bag.remove_all().is_err());
    }
}
