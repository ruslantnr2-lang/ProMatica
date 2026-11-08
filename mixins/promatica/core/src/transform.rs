//! Rotation and mirroring of positions and block states, as vanilla's `Rotation`/`Mirror` and
//! `BlockState.rotate`/`mirror` do (Litematica mirrors first, then rotates).

use crate::schematic::BlockState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    #[default]
    None,
    Clockwise90,
    Clockwise180,
    CounterClockwise90,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mirror {
    #[default]
    None,
    /// Flips z (vanilla `LEFT_RIGHT`).
    LeftRight,
    /// Flips x (vanilla `FRONT_BACK`).
    FrontBack,
}

impl Rotation {
    pub const ALL: [Rotation; 4] =
        [Rotation::None, Rotation::Clockwise90, Rotation::Clockwise180, Rotation::CounterClockwise90];
    pub fn quarter_turns(self) -> u8 {
        match self {
            Rotation::None => 0,
            Rotation::Clockwise90 => 1,
            Rotation::Clockwise180 => 2,
            Rotation::CounterClockwise90 => 3,
        }
    }
    pub fn from_quarter_turns(n: u8) -> Self {
        Self::ALL[(n % 4) as usize]
    }
    pub fn then(self, other: Rotation) -> Rotation {
        Self::from_quarter_turns(self.quarter_turns() + other.quarter_turns())
    }
    pub fn name(self) -> &'static str {
        match self {
            Rotation::None => "none",
            Rotation::Clockwise90 => "cw_90",
            Rotation::Clockwise180 => "180",
            Rotation::CounterClockwise90 => "ccw_90",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" | "0" => Rotation::None,
            "cw" | "cw_90" | "90" => Rotation::Clockwise90,
            "180" => Rotation::Clockwise180,
            "ccw" | "ccw_90" | "270" | "-90" => Rotation::CounterClockwise90,
            _ => return None,
        })
    }
    /// A horizontal offset rotated about the origin (`BlockPos.rotate`).
    pub fn apply(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Rotation::None => [x, y, z],
            Rotation::Clockwise90 => [-z, y, x],
            Rotation::Clockwise180 => [-x, y, -z],
            Rotation::CounterClockwise90 => [z, y, -x],
        }
    }
}

impl Mirror {
    pub fn name(self) -> &'static str {
        match self {
            Mirror::None => "none",
            Mirror::LeftRight => "left_right",
            Mirror::FrontBack => "front_back",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" => Mirror::None,
            "left_right" | "z" | "lr" => Mirror::LeftRight,
            "front_back" | "x" | "fb" => Mirror::FrontBack,
            _ => return None,
        })
    }
    pub fn apply(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Mirror::None => [x, y, z],
            Mirror::LeftRight => [x, y, -z],
            Mirror::FrontBack => [-x, y, z],
        }
    }
}

/// Horizontal and vertical directions by name.
fn dir_rotate(d: &str, r: Rotation) -> &str {
    const RING: [&str; 4] = ["north", "east", "south", "west"];
    match RING.iter().position(|&n| n == d) {
        Some(i) => RING[(i + r.quarter_turns() as usize) % 4],
        None => d,
    }
}

fn dir_mirror(d: &str, m: Mirror) -> &str {
    match (m, d) {
        (Mirror::LeftRight, "north") => "south",
        (Mirror::LeftRight, "south") => "north",
        (Mirror::FrontBack, "east") => "west",
        (Mirror::FrontBack, "west") => "east",
        _ => d,
    }
}

fn dir_axis(d: &str) -> char {
    match d {
        "north" | "south" => 'z',
        "east" | "west" => 'x',
        _ => 'y',
    }
}

/// Maps each `_`-separated direction token of a value (`ascending_east`, `north_up`, `south_west`).
fn map_tokens(v: &str, f: impl Fn(&str) -> String) -> String {
    v.split('_').map(f).collect::<Vec<_>>().join("_")
}

/// Rail shapes are written north-first (`north_east`, never `east_north`).
fn normalize_rail(v: String) -> String {
    let parts: Vec<&str> = v.split('_').collect();
    if parts.len() == 2 && parts[0] != "ascending" {
        let order = |d: &str| ["north", "south", "east", "west"].iter().position(|&n| n == d).unwrap_or(9);
        let (a, b) = (parts[0], parts[1]);
        if order(a) > order(b) {
            return format!("{b}_{a}");
        }
        // north_south / east_west keep their order whatever the rotation.
        if (a == "south" && b == "north") || (a == "west" && b == "east") {
            return format!("{b}_{a}");
        }
    }
    v
}

fn rail_fix(v: String) -> String {
    match v.as_str() {
        "south_north" => "north_south".into(),
        "west_east" => "east_west".into(),
        _ => normalize_rail(v),
    }
}

/// Rotates a state's properties.
pub fn rotate_state(state: &BlockState, r: Rotation) -> BlockState {
    if r == Rotation::None {
        return state.clone();
    }
    let mut out = state.clone();
    let turns = r.quarter_turns();
    let mut sides = Vec::new();
    for (k, v) in &state.properties {
        let new = match k.as_str() {
            "facing" | "horizontal_facing" | "vertical_direction" => dir_rotate(v, r).to_string(),
            "axis" if turns % 2 == 1 => match v.as_str() {
                "x" => "z".into(),
                "z" => "x".into(),
                _ => v.clone(),
            },
            "rotation" => match v.parse::<u32>() {
                Ok(n) => ((n + 4 * u32::from(turns)) % 16).to_string(),
                Err(_) => v.clone(),
            },
            "shape" if v.contains("north") || v.contains("south") || v.contains("east") || v.contains("west") => {
                rail_fix(map_tokens(v, |t| dir_rotate(t, r).to_string()))
            }
            "orientation" => map_tokens(v, |t| dir_rotate(t, r).to_string()),
            "north" | "east" | "south" | "west" => {
                sides.push((dir_rotate(k, r).to_string(), v.clone()));
                continue;
            }
            _ => continue,
        };
        out.properties.insert(k.clone(), new);
    }
    for (k, v) in sides {
        out.properties.insert(k, v);
    }
    out
}

/// Mirrors a state's properties.
pub fn mirror_state(state: &BlockState, m: Mirror) -> BlockState {
    if m == Mirror::None {
        return state.clone();
    }
    let mut out = state.clone();
    let mirror_axis = if m == Mirror::LeftRight { 'z' } else { 'x' };
    let mut sides = Vec::new();
    let facing = state.prop("facing").unwrap_or("");
    let is_stairs = state.name.ends_with("_stairs");
    for (k, v) in &state.properties {
        let new = match k.as_str() {
            "facing" | "horizontal_facing" => {
                if is_stairs && dir_axis(v) != mirror_axis {
                    // StairBlock.mirror leaves stairs facing across the mirror unchanged.
                    continue;
                }
                dir_mirror(v, m).to_string()
            }
            "rotation" => match v.parse::<i32>() {
                Ok(r) => {
                    // Mirror.mirror(rotation, 16).
                    let j = if r > 8 { r - 16 } else { r };
                    let n = match m {
                        Mirror::LeftRight => (8 - j + 16) % 16,
                        Mirror::FrontBack => (16 - j) % 16,
                        Mirror::None => r,
                    };
                    n.to_string()
                }
                Err(_) => v.clone(),
            },
            "shape" if is_stairs => {
                if dir_axis(facing) != mirror_axis {
                    continue;
                }
                match v.as_str() {
                    "inner_left" => "inner_right".into(),
                    "inner_right" => "inner_left".into(),
                    "outer_left" => "outer_right".into(),
                    "outer_right" => "outer_left".into(),
                    _ => continue,
                }
            }
            "shape" if v.contains("north") || v.contains("south") || v.contains("east") || v.contains("west") => {
                rail_fix(map_tokens(v, |t| dir_mirror(t, m).to_string()))
            }
            "orientation" => map_tokens(v, |t| dir_mirror(t, m).to_string()),
            "hinge" => match v.as_str() {
                "left" => "right".into(),
                "right" => "left".into(),
                _ => continue,
            },
            "type" if state.name.ends_with("chest") => match v.as_str() {
                "left" => "right".into(),
                "right" => "left".into(),
                _ => continue,
            },
            "north" | "east" | "south" | "west" => {
                sides.push((dir_mirror(k, m).to_string(), v.clone()));
                continue;
            }
            _ => continue,
        };
        out.properties.insert(k.clone(), new);
    }
    for (k, v) in sides {
        out.properties.insert(k, v);
    }
    out
}

/// Mirror, then rotation (Litematica's order).
pub fn transform_state(state: &BlockState, m: Mirror, r: Rotation) -> BlockState {
    rotate_state(&mirror_state(state, m), r)
}

pub fn transform_pos(p: [i32; 3], m: Mirror, r: Rotation) -> [i32; 3] {
    r.apply(m.apply(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(name: &str, props: &[(&str, &str)]) -> BlockState {
        BlockState::new(name, props)
    }

    #[test]
    fn positions() {
        assert_eq!(Rotation::Clockwise90.apply([1, 5, 0]), [0, 5, 1]);
        assert_eq!(Rotation::Clockwise90.apply([0, 0, 1]), [-1, 0, 0]);
        assert_eq!(transform_pos([1, 0, 2], Mirror::LeftRight, Rotation::Clockwise180), [-1, 0, 2]);
        for r in Rotation::ALL {
            let p = [3, 1, -7];
            let mut q = p;
            for _ in 0..4 {
                q = r.apply(q);
            }
            assert_eq!(p, q);
        }
    }

    #[test]
    fn facing_and_sides() {
        let s = st("minecraft:furnace", &[("facing", "north"), ("lit", "false")]);
        assert_eq!(rotate_state(&s, Rotation::Clockwise90).prop("facing"), Some("east"));
        assert_eq!(mirror_state(&s, Mirror::LeftRight).prop("facing"), Some("south"));
        assert_eq!(mirror_state(&s, Mirror::FrontBack).prop("facing"), Some("north"));
        let f = st("minecraft:oak_fence", &[("north", "true"), ("east", "false"), ("south", "false"), ("west", "false")]);
        let r = rotate_state(&f, Rotation::Clockwise90);
        assert_eq!(r.prop("east"), Some("true"));
        assert_eq!(r.prop("north"), Some("false"));
    }

    #[test]
    fn axis_rotation_and_rails() {
        let log = st("minecraft:oak_log", &[("axis", "x")]);
        assert_eq!(rotate_state(&log, Rotation::Clockwise90).prop("axis"), Some("z"));
        assert_eq!(rotate_state(&log, Rotation::Clockwise180).prop("axis"), Some("x"));
        let sign = st("minecraft:oak_sign", &[("rotation", "0")]);
        assert_eq!(rotate_state(&sign, Rotation::Clockwise90).prop("rotation"), Some("4"));
        assert_eq!(mirror_state(&sign, Mirror::LeftRight).prop("rotation"), Some("8"));
        assert_eq!(mirror_state(&st("minecraft:oak_sign", &[("rotation", "4")]), Mirror::FrontBack).prop("rotation"), Some("12"));
        let rail = st("minecraft:rail", &[("shape", "north_east")]);
        assert_eq!(rotate_state(&rail, Rotation::Clockwise90).prop("shape"), Some("south_east"));
        assert_eq!(rotate_state(&rail, Rotation::Clockwise180).prop("shape"), Some("south_west"));
        let straight = st("minecraft:rail", &[("shape", "north_south")]);
        assert_eq!(rotate_state(&straight, Rotation::Clockwise90).prop("shape"), Some("east_west"));
        let up = st("minecraft:rail", &[("shape", "ascending_north")]);
        assert_eq!(rotate_state(&up, Rotation::CounterClockwise90).prop("shape"), Some("ascending_west"));
    }

    #[test]
    fn stairs_doors_chests() {
        let s = st("minecraft:oak_stairs", &[("facing", "north"), ("shape", "inner_left"), ("half", "bottom")]);
        let m = mirror_state(&s, Mirror::LeftRight);
        assert_eq!(m.prop("facing"), Some("south"));
        assert_eq!(m.prop("shape"), Some("inner_right"));
        let across = mirror_state(&s, Mirror::FrontBack);
        assert_eq!(across, s);
        let door = st("minecraft:oak_door", &[("facing", "east"), ("hinge", "left")]);
        let d = mirror_state(&door, Mirror::FrontBack);
        assert_eq!((d.prop("facing"), d.prop("hinge")), (Some("west"), Some("right")));
        let chest = st("minecraft:chest", &[("facing", "north"), ("type", "left")]);
        assert_eq!(mirror_state(&chest, Mirror::LeftRight).prop("type"), Some("right"));
    }
}
