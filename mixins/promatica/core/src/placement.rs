//! A schematic placed in the world: origin, mirror and rotation, the transformed palettes, and
//! the reverse lookup from a world position to the block the schematic wants there.

use crate::schematic::{BlockState, Schematic};
use crate::transform::{Mirror, Rotation, mirror_state, rotate_state, transform_pos};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerMode {
    #[default]
    All,
    /// Only the layer at `y`.
    Single(i32),
    /// `y` and everything above it.
    Above(i32),
    /// `y` and everything below it.
    Below(i32),
    /// From `min` to `max` inclusive.
    Range(i32, i32),
}

impl LayerMode {
    pub fn contains(self, y: i32) -> bool {
        match self {
            LayerMode::All => true,
            LayerMode::Single(l) => y == l,
            LayerMode::Above(l) => y >= l,
            LayerMode::Below(l) => y <= l,
            LayerMode::Range(a, b) => y >= a.min(b) && y <= a.max(b),
        }
    }
    /// Moves the layer(s) by `dy`.
    pub fn shifted(self, dy: i32) -> Self {
        match self {
            LayerMode::All => LayerMode::All,
            LayerMode::Single(l) => LayerMode::Single(l + dy),
            LayerMode::Above(l) => LayerMode::Above(l + dy),
            LayerMode::Below(l) => LayerMode::Below(l + dy),
            LayerMode::Range(a, b) => LayerMode::Range(a + dy, b + dy),
        }
    }
    pub fn describe(self) -> String {
        match self {
            LayerMode::All => "all".into(),
            LayerMode::Single(l) => format!("layer {l}"),
            LayerMode::Above(l) => format!("y >= {l}"),
            LayerMode::Below(l) => format!("y <= {l}"),
            LayerMode::Range(a, b) => format!("y {}..{}", a.min(b), a.max(b)),
        }
    }
}

/// A loaded schematic and where it stands in the world.
pub struct Placement {
    pub file_name: String,
    pub schematic: Schematic,
    pub origin: [i32; 3],
    pub rotation: Rotation,
    pub mirror: Mirror,
    /// Per region, the palette after mirror and rotation.
    palettes: Vec<Vec<BlockState>>,
    /// World-space box (min, max inclusive), recomputed on every change.
    bounds: ([i32; 3], [i32; 3]),
    /// Bumped on every change so caches built from the placement know to rebuild.
    pub revision: u64,
}

impl Placement {
    pub fn new(file_name: String, schematic: Schematic, origin: [i32; 3]) -> Self {
        let mut p = Self {
            file_name,
            schematic,
            origin,
            rotation: Rotation::None,
            mirror: Mirror::None,
            palettes: Vec::new(),
            bounds: ([0; 3], [0; 3]),
            revision: 0,
        };
        p.update();
        p
    }

    fn update(&mut self) {
        self.palettes = self
            .schematic
            .regions
            .iter()
            .map(|r| {
                r.palette
                    .iter()
                    .map(|s| rotate_state(&mirror_state(s, self.mirror), self.rotation))
                    .collect()
            })
            .collect();
        let (lo, hi) = self.schematic.bounds();
        let mut wlo = [i32::MAX; 3];
        let mut whi = [i32::MIN; 3];
        for x in [lo[0], hi[0]] {
            for y in [lo[1], hi[1]] {
                for z in [lo[2], hi[2]] {
                    let w = self.to_world([x, y, z]);
                    for i in 0..3 {
                        wlo[i] = wlo[i].min(w[i]);
                        whi[i] = whi[i].max(w[i]);
                    }
                }
            }
        }
        self.bounds = (wlo, whi);
        self.revision += 1;
    }

    pub fn set_origin(&mut self, origin: [i32; 3]) {
        self.origin = origin;
        self.update();
    }
    pub fn nudge(&mut self, d: [i32; 3]) {
        self.origin = [0, 1, 2].map(|i| self.origin[i] + d[i]);
        self.update();
    }
    pub fn set_rotation(&mut self, r: Rotation) {
        self.rotation = r;
        self.update();
    }
    pub fn set_mirror(&mut self, m: Mirror) {
        self.mirror = m;
        self.update();
    }

    /// Schematic-relative position → world position.
    pub fn to_world(&self, rel: [i32; 3]) -> [i32; 3] {
        let t = transform_pos(rel, self.mirror, self.rotation);
        [0, 1, 2].map(|i| self.origin[i] + t[i])
    }

    /// World position → schematic-relative position (the inverse of `to_world`).
    pub fn to_schematic(&self, world: [i32; 3]) -> [i32; 3] {
        let d = [0, 1, 2].map(|i| world[i] - self.origin[i]);
        let inverse = Rotation::from_quarter_turns(4 - self.rotation.quarter_turns());
        self.mirror.apply(inverse.apply(d))
    }

    pub fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        self.bounds
    }

    pub fn in_bounds(&self, w: [i32; 3]) -> bool {
        let (lo, hi) = self.bounds;
        (0..3).all(|i| w[i] >= lo[i] && w[i] <= hi[i])
    }

    /// Where the schematic's block for world position `w` is: (region, index in its palette).
    /// None outside every region and on structure void (anything goes there). The game side
    /// keys its caches by this instead of hashing the state.
    pub fn wanted_ref(&self, w: [i32; 3]) -> Option<(usize, u32)> {
        if !self.in_bounds(w) {
            return None;
        }
        let rel = self.to_schematic(w);
        let mut found: Option<(usize, u32)> = None;
        for (ri, r) in self.schematic.regions.iter().enumerate() {
            let local = [0, 1, 2].map(|i| rel[i] - r.min[i]);
            if (0..3).all(|i| local[i] >= 0 && local[i] < r.size[i]) {
                let pi = r.palette_index(local);
                // A later region's air doesn't erase an earlier region's block.
                if found.is_none() || !self.palettes[ri][pi as usize].is_air() {
                    found = Some((ri, pi));
                }
            }
        }
        found.filter(|&(ri, pi)| !self.palettes[ri][pi as usize].is_void())
    }

    /// The (transformed) state at a [`wanted_ref`](Self::wanted_ref).
    pub fn palette_state(&self, (ri, pi): (usize, u32)) -> &BlockState {
        &self.palettes[ri][pi as usize]
    }

    /// The (transformed) state the schematic wants at world position `w`, or None outside every
    /// region. Air inside a region is `Some(air)`; structure void is None (anything goes).
    pub fn wanted(&self, w: [i32; 3]) -> Option<&BlockState> {
        self.wanted_ref(w).map(|r| self.palette_state(r))
    }

    /// Every non-air block of the placement whose world position satisfies `keep`, region by
    /// region: (world position, transformed state).
    pub fn for_each_block(&self, mut keep: impl FnMut([i32; 3]) -> bool, mut f: impl FnMut([i32; 3], &BlockState)) {
        for (ri, r) in self.schematic.regions.iter().enumerate() {
            let palette = &self.palettes[ri];
            let solid: Vec<bool> = palette.iter().map(|s| !s.is_air() && !s.is_void()).collect();
            for y in 0..r.size[1] {
                for z in 0..r.size[2] {
                    for x in 0..r.size[0] {
                        let i = r.palette_index([x, y, z]) as usize;
                        if !solid[i] {
                            continue;
                        }
                        let w = self.to_world([r.min[0] + x, r.min[1] + y, r.min[2] + z]);
                        if keep(w) {
                            f(w, &palette[i]);
                        }
                    }
                }
            }
        }
    }

    /// The transformed palettes (for counting materials without walking every cell).
    pub fn palettes(&self) -> &[Vec<BlockState>] {
        &self.palettes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schematic::{Metadata, Region};

    fn line() -> Placement {
        // Three blocks along +x: stone, dirt, a furnace facing north.
        let region = Region::from_fn("r", [0, 0, 0], [3, 1, 1], |[x, _, _]| match x {
            0 => BlockState::new("minecraft:stone", &[]),
            1 => BlockState::new("minecraft:dirt", &[]),
            _ => BlockState::new("minecraft:furnace", &[("facing", "north")]),
        });
        let s = Schematic { version: 6, data_version: 0, metadata: Metadata::default(), regions: vec![region] };
        Placement::new("line".into(), s, [10, 64, 10])
    }

    #[test]
    fn world_lookup_follows_rotation() {
        let mut p = line();
        assert_eq!(p.wanted([12, 64, 10]).unwrap().name, "minecraft:furnace");
        p.set_rotation(Rotation::Clockwise90);
        // +x becomes +z.
        let f = p.wanted([10, 64, 12]).unwrap();
        assert_eq!((f.name.as_str(), f.prop("facing")), ("minecraft:furnace", Some("east")));
        assert!(p.wanted([12, 64, 10]).is_none());
        assert_eq!(p.bounds(), ([10, 64, 10], [10, 64, 12]));
    }

    #[test]
    fn inverse_is_exact() {
        let mut p = line();
        for m in [Mirror::None, Mirror::LeftRight, Mirror::FrontBack] {
            for r in Rotation::ALL {
                p.set_mirror(m);
                p.set_rotation(r);
                for rel in [[0, 0, 0], [2, 3, -4], [-5, 1, 7]] {
                    assert_eq!(p.to_schematic(p.to_world(rel)), rel, "{m:?} {r:?}");
                }
            }
        }
    }

    #[test]
    fn iterates_blocks_in_world_space() {
        let mut p = line();
        p.set_mirror(Mirror::FrontBack);
        let mut seen = Vec::new();
        p.for_each_block(|_| true, |w, s| seen.push((w, s.name.clone())));
        assert_eq!(seen.len(), 3);
        assert!(seen.contains(&([8, 64, 10], "minecraft:furnace".to_string())));
    }

    #[test]
    fn layers() {
        assert!(LayerMode::Range(5, 3).contains(4));
        assert!(!LayerMode::Single(4).contains(5));
        assert_eq!(LayerMode::Above(3).shifted(2), LayerMode::Above(5));
    }
}
