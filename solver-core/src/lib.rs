//! Shapeshifter solver core.
//!
//! The puzzle: a grid of cells, each in one of k cyclic states; a fixed
//! sequence of shapes, each of which must be stamped onto the board exactly
//! once, advancing every covered cell by one state. Find placements that
//! leave every cell in the goal state.

pub mod engine;

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

/// Cooperative cancellation for embedders: set true to abort a running solve.
pub static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolverInput {
    pub width: usize,
    pub height: usize,
    /// Row-major cell states, 0-based indices into the state cycle.
    pub grid: Vec<u8>,
    /// Target state (index into the cycle).
    pub goal: u8,
    /// Number of states in the cycle. 0 = derive from the grid maximum
    /// (unreliable when the highest state is absent from the board —
    /// always send this explicitly).
    #[serde(default)]
    pub num_states: u8,
    pub shapes: Vec<ShapeData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeData {
    pub id: usize,
    /// Cell offsets within the shape's bounding box, as row * width + col
    /// (normalized: some cell in row 0, some cell in column 0).
    pub points: Vec<usize>,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct SolutionStep {
    pub original_shape_id: usize,
    /// Column of the shape's bounding-box top-left — the cell to click.
    pub placement_x: usize,
    /// Row of the shape's bounding-box top-left.
    pub placement_y: usize,
}
