//! Flicker in a recording of every presented frame: parts of the picture that change for a few frames and then
//! show what they showed before, while the rest of the scene holds still.

use serde::{Deserialize, Serialize};

use crate::picture::{ColourGrid, GRID_HEIGHT, GRID_WIDTH};

/// A channel that moves this much or more makes a cell differ, 0…255.
pub const CHANGE: f64 = 32.0;
/// A cell is back when no channel is further than this from the frame before the change, 0…255.
pub const STEADY: f64 = 12.0;
/// The longest change, in frames, that still counts as flicker.
pub const MAX_RUN: usize = 3;

/// A part of the picture that changed for a few frames and came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flicker {
    /// The first frame that differs, counted from 0.
    pub first: usize,
    /// How many frames differ before the picture is back.
    pub frames: usize,
    /// How many cells of the 64×36 grid flickered.
    pub cells: usize,
    /// The flickering cells' bounds: left, top, right and bottom cell, inclusive.
    pub area: [usize; 4],
}

/// Returns every flicker in `frames`, in frame order; a flicker of more than [`MAX_RUN`] frames is a change of
/// the scene and is not returned.
#[must_use]
pub fn find(frames: &[ColourGrid]) -> Vec<Flicker> {
    let mut found = Vec::new();
    let mut start = 1;
    while start + 1 < frames.len() {
        let run = (1..=MAX_RUN)
            .filter(|run| start + run < frames.len())
            .find_map(|run| flicker_at(frames, start, run));
        match run {
            Some(flicker) => {
                start += flicker.frames;
                found.push(flicker);
            }
            None => start += 1,
        }
    }
    found
}

fn flicker_at(frames: &[ColourGrid], start: usize, run: usize) -> Option<Flicker> {
    let before = &frames[start - 1];
    let after = &frames[start + run];
    let mut cells = 0;
    let mut area = [usize::MAX, usize::MAX, 0, 0];
    for index in 0..GRID_WIDTH * GRID_HEIGHT {
        if before.cell_difference(after, index) > STEADY {
            continue;
        }
        let changed = frames[start..start + run].iter().all(|frame| {
            frame.cell_difference(before, index) >= CHANGE
                && frame.cell_difference(after, index) >= CHANGE
        });
        if !changed {
            continue;
        }
        let (x, y) = (index % GRID_WIDTH, index / GRID_WIDTH);
        cells += 1;
        area = [
            area[0].min(x),
            area[1].min(y),
            area[2].max(x),
            area[3].max(y),
        ];
    }
    (cells > 0).then_some(Flicker {
        first: start,
        frames: run,
        cells,
        area,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(colour: [f64; 3]) -> ColourGrid {
        ColourGrid::from_cells(vec![colour; GRID_WIDTH * GRID_HEIGHT])
    }

    fn with_cell(base: [f64; 3], index: usize, colour: [f64; 3]) -> ColourGrid {
        let mut cells = vec![base; GRID_WIDTH * GRID_HEIGHT];
        cells[index] = colour;
        ColourGrid::from_cells(cells)
    }

    #[test]
    fn a_still_scene_has_no_flicker() {
        let frames = vec![grid([90.0, 60.0, 40.0]); 6];
        assert!(find(&frames).is_empty());
    }

    #[test]
    fn one_frame_that_drops_a_texture_is_flicker() {
        let wood = [120.0, 80.0, 50.0];
        let frames = vec![
            grid(wood),
            grid(wood),
            with_cell(wood, 70, [0.0, 0.0, 0.0]),
            grid(wood),
            grid(wood),
        ];
        assert_eq!(
            find(&frames),
            vec![Flicker {
                first: 2,
                frames: 1,
                cells: 1,
                area: [6, 1, 6, 1],
            }]
        );
    }

    #[test]
    fn red_and_blue_exchanged_for_two_frames_is_flicker() {
        let red = [200.0, 30.0, 30.0];
        let blue = [30.0, 30.0, 200.0];
        let frames = vec![
            grid(red),
            with_cell(red, 0, blue),
            with_cell(red, 0, blue),
            grid(red),
        ];
        let found = find(&frames);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].first, found[0].frames, found[0].cells), (1, 2, 1));
    }

    #[test]
    fn a_lasting_change_is_not_flicker() {
        let frames = vec![
            grid([20.0, 20.0, 20.0]),
            grid([200.0, 200.0, 200.0]),
            grid([200.0, 200.0, 200.0]),
            grid([200.0, 200.0, 200.0]),
            grid([200.0, 200.0, 200.0]),
            grid([200.0, 200.0, 200.0]),
        ];
        assert!(find(&frames).is_empty());
    }

    #[test]
    fn a_small_change_is_not_flicker() {
        let base = [100.0, 100.0, 100.0];
        let frames = vec![
            grid(base),
            with_cell(base, 5, [120.0, 110.0, 100.0]),
            grid(base),
        ];
        assert!(find(&frames).is_empty());
    }
}
