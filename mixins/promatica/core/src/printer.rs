//! Planning for the printer (auto-build): which positions to try, in which order, and how to
//! keep from clicking the same spot again before the world answers.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrinterSettings {
    /// Blocks placed at most per second-ish: one placement every `interval` ticks.
    pub interval: u32,
    /// Reach in blocks from the eye.
    pub reach: f64,
    /// Ticks before a position clicked once may be clicked again.
    pub retry_ticks: u64,
    /// Also replace wrong blocks? (No: the printer only places into missing spots.)
    pub only_layer: bool,
    /// Click the empty spot itself when no neighbour can be clicked ("air place").
    pub air_place: bool,
}

impl Default for PrinterSettings {
    fn default() -> Self {
        Self { interval: 2, reach: 4.5, retry_ticks: 20, only_layer: true, air_place: false }
    }
}

/// Integer positions whose centre lies within `reach` of `eye`, bottom layer first, then nearest
/// first: building upwards keeps supports below what comes next.
pub fn positions_in_reach(eye: [f64; 3], reach: f64) -> Vec<[i32; 3]> {
    let r = reach.ceil() as i32 + 1;
    let base = eye.map(|v| v.floor() as i32);
    let mut out = Vec::new();
    for dy in -r..=r {
        for dz in -r..=r {
            for dx in -r..=r {
                let p = [base[0] + dx, base[1] + dy, base[2] + dz];
                if dist2_center(eye, p) <= reach * reach {
                    out.push(p);
                }
            }
        }
    }
    out.sort_by(|a, b| a[1].cmp(&b[1]).then(dist2_center(eye, *a).total_cmp(&dist2_center(eye, *b))));
    out
}

pub fn dist2_center(eye: [f64; 3], p: [i32; 3]) -> f64 {
    (0..3).map(|i| (f64::from(p[i]) + 0.5 - eye[i]).powi(2)).sum()
}

/// Positions clicked recently, so a slow server's echo doesn't make the printer click twice.
#[derive(Default)]
pub struct Recent {
    at: HashMap<[i32; 3], u64>,
}

impl Recent {
    pub fn blocked(&self, p: [i32; 3], now: u64, retry: u64) -> bool {
        self.at.get(&p).is_some_and(|&t| now < t + retry)
    }
    pub fn mark(&mut self, p: [i32; 3], now: u64) {
        self.at.insert(p, now);
        if self.at.len() > 4096 {
            self.at.retain(|_, &mut t| t + 200 > now);
        }
    }
    pub fn clear(&mut self) {
        self.at.clear();
    }
}

/// Blocks no click could place, and what they looked like then. Nothing is tried again until
/// something that matters changed (`key`: where the player stands, the block and its neighbours),
/// or after `STUCK_TICKS` for what `key` can't see, like a block taken away in the line of sight.
#[derive(Default)]
pub struct Stuck {
    at: HashMap<[i32; 3], (u64, u64)>,
}

pub const STUCK_TICKS: u64 = 200;

impl Stuck {
    pub fn holds(&self, p: [i32; 3], key: u64, now: u64) -> bool {
        self.at.get(&p).is_some_and(|&(k, t)| k == key && now < t + STUCK_TICKS)
    }
    pub fn mark(&mut self, p: [i32; 3], key: u64, now: u64) {
        self.at.insert(p, (key, now));
        if self.at.len() > 4096 {
            self.at.retain(|_, &mut (_, t)| now < t + STUCK_TICKS);
        }
    }
    pub fn clear(&mut self) {
        self.at.clear();
    }
}

/// Whether the eye can see the face that the neighbour at `p + f` turns to `p`, within `reach`: it
/// has to be on `p`'s side of that face. No ray is needed to know the others can't be clicked.
pub fn face_in_view(eye: [f64; 3], reach: f64, p: [i32; 3], f: [i32; 3]) -> bool {
    let Some(axis) = f.iter().position(|&v| v != 0) else { return false };
    let plane = f64::from(p[axis]) + if f[axis] > 0 { 1.0 } else { 0.0 };
    let front = if f[axis] > 0 { eye[axis] < plane } else { eye[axis] > plane };
    // The nearest point of the face.
    let dist2: f64 = (0..3)
        .map(|i| {
            let d = if i == axis {
                plane - eye[i]
            } else {
                eye[i] - eye[i].clamp(f64::from(p[i]), f64::from(p[i]) + 1.0)
            };
            d * d
        })
        .sum();
    front && dist2 <= reach * reach
}

/// The six faces as (offset to the neighbour).
pub const FACES: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

/// Candidate yaw/pitch pairs to try when a block's placement depends on where the player looks:
/// the four cardinal yaws at level, up and down pitches. Vanilla reads `getHorizontalDirection`
/// and `getNearestLookingDirection`, so these cover every facing a player can give.
pub fn look_candidates() -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for yaw in [0.0, 90.0, 180.0, -90.0] {
        for pitch in [0.0, 60.0, -60.0, 89.0, -89.0] {
            out.push((yaw, pitch));
        }
    }
    out
}

/// Whether a click on face `f` can give `state` its direction: a log, froglight or other pillar
/// takes its `axis` from the clicked face alone, whatever the look.
pub fn face_fits(state: &crate::BlockState, f: [i32; 3]) -> bool {
    match state.prop("axis") {
        Some("x") => f[0] != 0,
        Some("y") => f[1] != 0,
        Some("z") => f[2] != 0,
        _ => true,
    }
}

/// When the printer places a block, so that redstone doesn't fire while it is built: plain
/// blocks first (0), then what carries or uses a signal (1), then what makes one (2), and
/// observers last (3), each once the block it watches is there.
pub fn place_class(state: &crate::BlockState) -> u8 {
    let name = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
    const SOURCES: &[&str] = &[
        "redstone_block", "redstone_torch", "redstone_wall_torch", "lever", "daylight_detector", "target",
        "tripwire_hook", "tripwire", "trapped_chest", "lectern", "sculk_sensor", "calibrated_sculk_sensor",
        "lightning_rod", "detector_rail",
    ];
    const SIGNAL: &[&str] = &[
        "redstone_wire", "repeater", "comparator", "piston", "sticky_piston", "dispenser", "dropper", "hopper",
        "note_block", "redstone_lamp", "powered_rail", "activator_rail", "tnt", "crafter", "bell", "copper_bulb",
    ];
    if name == "observer" {
        3
    } else if SOURCES.contains(&name) || name.ends_with("_button") || name.ends_with("_pressure_plate") {
        2
    } else if SIGNAL.contains(&name)
        || name.ends_with("copper_bulb")
        || name.ends_with("_door")
        || name.ends_with("_trapdoor")
        || name.ends_with("_fence_gate")
    {
        1
    } else {
        0
    }
}

/// Blocks that fall when nothing holds them up (`FallingBlock`): sand, gravel, concrete powder,
/// anvils, the dragon egg. The printer places them only on something.
pub fn falls(state: &crate::BlockState) -> bool {
    let name = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
    matches!(
        name,
        "sand" | "red_sand" | "suspicious_sand" | "gravel" | "suspicious_gravel" | "anvil" | "chipped_anvil"
            | "damaged_anvil" | "dragon_egg"
    ) || name.ends_with("_concrete_powder")
}

/// The block an observer watches, relative to it (its `facing`).
pub fn observed(state: &crate::BlockState) -> Option<[i32; 3]> {
    if state.name != "minecraft:observer" {
        return None;
    }
    Some(match state.prop("facing")? {
        "up" => [0, 1, 0],
        "down" => [0, -1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        _ => [1, 0, 0],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_order() {
        let p = positions_in_reach([0.5, 64.6, 0.5], 4.5);
        assert!(p.windows(2).all(|w| w[0][1] <= w[1][1]));
        assert!(p.iter().all(|&q| dist2_center([0.5, 64.6, 0.5], q) <= 4.5 * 4.5));
        assert!(p.contains(&[0, 60, 0]));
        assert!(!p.contains(&[0, 59, 0]));
    }

    #[test]
    fn recent_blocks_until_retry() {
        let mut r = Recent::default();
        r.mark([1, 2, 3], 100);
        assert!(r.blocked([1, 2, 3], 110, 20));
        assert!(!r.blocked([1, 2, 3], 120, 20));
        assert!(!r.blocked([0, 0, 0], 110, 20));
    }

    #[test]
    fn stuck_until_something_changes() {
        let mut s = Stuck::default();
        s.mark([1, 2, 3], 7, 100);
        assert!(s.holds([1, 2, 3], 7, 150));
        assert!(!s.holds([1, 2, 3], 8, 150), "something around it changed");
        assert!(!s.holds([1, 2, 3], 7, 100 + STUCK_TICKS), "tried again in time");
        assert!(!s.holds([0, 0, 0], 7, 150));
    }

    #[test]
    fn faces_the_eye_can_see() {
        let p = [0, 64, 0];
        let down = [0, -1, 0];
        // The top of the block below: seen from above, not from below it.
        assert!(face_in_view([0.5, 66.0, 0.5], 4.5, p, down));
        assert!(!face_in_view([0.5, 63.0, 0.5], 4.5, p, down));
        // The block to the east turns its west side to `p`: seen from the west only.
        assert!(face_in_view([-2.0, 64.5, 0.5], 4.5, p, [1, 0, 0]));
        assert!(!face_in_view([3.0, 64.5, 0.5], 4.5, p, [1, 0, 0]));
        // Too far.
        assert!(!face_in_view([0.5, 74.0, 0.5], 4.5, p, down));
    }

    #[test]
    fn pillars_take_their_axis_from_the_face() {
        let pillar = |a: &str| crate::BlockState::new("minecraft:verdant_froglight", &[("axis", a)]);
        assert!(face_fits(&pillar("y"), [0, -1, 0]));
        assert!(!face_fits(&pillar("y"), [1, 0, 0]));
        assert!(face_fits(&pillar("z"), [0, 0, 1]));
        assert!(!face_fits(&pillar("z"), [0, -1, 0]));
        assert!(face_fits(&crate::BlockState::new("minecraft:stone", &[]), [1, 0, 0]));
    }

    #[test]
    fn redstone_comes_late() {
        let b = |n: &str| crate::BlockState::new(n, &[]);
        assert_eq!(place_class(&b("minecraft:stone")), 0);
        assert_eq!(place_class(&b("minecraft:repeater")), 1);
        assert_eq!(place_class(&b("minecraft:redstone_torch")), 2);
        assert_eq!(place_class(&b("minecraft:stone_button")), 2);
        assert_eq!(place_class(&b("minecraft:observer")), 3);
        let obs = crate::BlockState::new("minecraft:observer", &[("facing", "west")]);
        assert_eq!(observed(&obs), Some([-1, 0, 0]));
    }

    #[test]
    fn falling_blocks() {
        let b = |n: &str| crate::BlockState::new(n, &[]);
        assert!(falls(&b("minecraft:sand")));
        assert!(falls(&b("minecraft:damaged_anvil")));
        assert!(falls(&b("minecraft:lime_concrete_powder")));
        assert!(!falls(&b("minecraft:lime_concrete")));
        assert!(!falls(&b("minecraft:sandstone")));
    }
}
