//! Middle-layer (rest area) board parsing.
//!
//! `rouge_middle_layer` ships a hand-placed board:
//! - `pointPos`    `x#y#index|...` — walkable points and their index
//! - `path`        `a#b|a#b|...`   — undirected edges between point indices
//! - `leavePos`    `x#y#index`     — the exit point
//! - `nextLayer`   `121#122|...`   — layers reachable after leaving
//!
//! The client only renders these positions, so the server keeps the graph.

use crate::error::AppError;
use config::rouge_middle_layer::RougeMiddleLayer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiddleBoard {
    pub id: i32,
    pub points: Vec<i32>,
    pub edges: Vec<(i32, i32)>,
    pub entry: i32,
    pub leave: i32,
    pub next_layers: Vec<i32>,
}

impl MiddleBoard {
    pub fn adjacent(&self, from: i32, to: i32) -> bool {
        self.edges
            .iter()
            .any(|(a, b)| (*a == from && *b == to) || (*a == to && *b == from))
    }

    pub fn can_leave(&self, position: i32) -> bool {
        position == self.leave
    }
}

fn parse_point_index(raw: &str) -> Option<i32> {
    raw.rsplit('#').next()?.trim().parse().ok()
}

/// Parses the board for a middle-layer row.
pub fn parse(row: &RougeMiddleLayer) -> Result<MiddleBoard, AppError> {
    let points: Vec<i32> = row
        .point_pos
        .split('|')
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(parse_point_index)
        .collect();
    if points.is_empty() {
        return Err(AppError::InvalidRequest);
    }

    let edges: Vec<(i32, i32)> = row
        .path
        .split('|')
        .filter_map(|entry| {
            let (a, b) = entry.split_once('#')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .collect();
    if edges.is_empty() {
        return Err(AppError::InvalidRequest);
    }

    // The final safe zone has a boss piece, with no exit or following layer.
    let terminal = row.leave_pos.is_empty() && row.next_layer.is_empty();
    let leave = if terminal {
        -1
    } else {
        parse_point_index(&row.leave_pos).ok_or(AppError::InvalidRequest)?
    };
    if !terminal
        && !points.contains(&leave)
        && !edges.iter().any(|(a, b)| *a == leave || *b == leave)
    {
        return Err(AppError::InvalidRequest);
    }

    // Entry: the walkable point that is never a path target and is not the exit.
    // `path` is undirected, so a point that only appears as a source is a leaf.
    let entry = points
        .iter()
        .copied()
        .find(|point| *point != leave && !edges.iter().any(|(_, b)| b == point))
        .or_else(|| points.iter().copied().find(|point| *point != leave))
        .ok_or(AppError::InvalidRequest)?;

    let next_layers = row
        .next_layer
        .split('#')
        .filter_map(|entry| entry.trim().parse().ok())
        .collect();

    Ok(MiddleBoard {
        id: row.id,
        points,
        edges,
        entry,
        leave,
        next_layers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> RougeMiddleLayer {
        // Mirrors rouge_middle_layer 111 from the shipped data.
        RougeMiddleLayer {
            id: 111,
            day_or_night: 1,
            empty: 0,
            leave_pos: "9.41#1.53#8".to_string(),
            leave_pos_unlock_param: String::new(),
            leave_pos_unlock_type: 0,
            map_res: String::new(),
            name: String::new(),
            next_layer: "121#122".to_string(),
            path: "4#1|4#5|3#4|3#7|6#7|2#3|8#6".to_string(),
            path_point_pos: String::new(),
            path_select: "111".to_string(),
            point_pos:
                "0.47#-0.92#3|-2.92#-2.24#1|2.7#2.52#2|-0.43#-5.08#5|7.63#-1.51#6|3.55#-0.15#7"
                    .to_string(),
            version: String::new(),
        }
    }

    #[test]
    fn parses_points_edges_entry_and_exit() {
        let board = parse(&row()).unwrap();
        assert_eq!(board.points, vec![3, 1, 2, 5, 6, 7]);
        assert_eq!(board.edges.len(), 7);
        assert_eq!(board.leave, 8);
        assert_eq!(board.next_layers, vec![121, 122]);
        // Point 2 only ever appears as a source, so it is the entry.
        assert_eq!(board.entry, 2);
    }

    #[test]
    fn adjacency_is_undirected_and_exit_requires_the_leave_point() {
        let board = parse(&row()).unwrap();
        assert!(board.adjacent(2, 3));
        assert!(board.adjacent(3, 2));
        assert!(!board.adjacent(2, 7));
        assert!(board.can_leave(8));
        assert!(!board.can_leave(3));
    }
}
