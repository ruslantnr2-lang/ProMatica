//! The verifier's classification of a world block against the schematic's (`SchematicVerifier`),
//! and Litematica's overlay colours.

use crate::schematic::BlockState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    /// The world already holds the wanted block.
    Correct,
    /// The schematic wants a block; the world has air (or a replaceable plant, fluid).
    Missing,
    /// The schematic wants air; the world has a block.
    Extra,
    /// Another block.
    WrongBlock,
    /// The right block in another state (facing, half...).
    WrongState,
}

impl Status {
    /// `schematicOverlayColor*` defaults (ARGB).
    pub fn overlay_argb(self) -> u32 {
        match self {
            Status::Correct => 0x2011_FF11,
            Status::Missing => 0x2C33_B3E6,
            Status::Extra => 0x4CFF_4CE6,
            Status::WrongBlock => 0x4CFF_3333,
            Status::WrongState => 0x4CFF_9010,
        }
    }
    /// `MismatchType` text colours (RGB).
    pub fn text_rgb(self) -> u32 {
        match self {
            Status::Correct => 0x11FF11,
            Status::Missing => 0x00FFFF,
            Status::Extra => 0xFF00CF,
            Status::WrongBlock => 0xFF0000,
            Status::WrongState => 0xFFAF00,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Status::Correct => "correct",
            Status::Missing => "missing",
            Status::Extra => "extra",
            Status::WrongBlock => "wrong block",
            Status::WrongState => "wrong state",
        }
    }
}

/// Properties that don't count as a wrong state: they follow neighbours or time, not the build.
const IGNORED_PROPS: &[&str] = &["waterlogged", "distance", "persistent", "age", "leaves", "moisture", "stage", "honey_level", "occupied", "open", "has_book", "has_record", "bites", "level", "eye", "in_wall", "locked", "signal_fire", "unstable", "drag", "berries", "short", "attached", "disarmed", "hatch"];

/// Redstone's on/off: wrong only when asked for (`power`), and never in the printer's way.
const POWER_PROPS: &[&str] = &["power", "powered", "triggered", "lit", "extended", "enabled"];

/// What a placed block is clicked to (a repeater's delay, a comparator's mode, a daylight
/// detector's inversion, a note block's note): the printer sets them after placing.
pub const CONFIG_PROPS: &[&str] = &["delay", "mode", "inverted", "note"];

/// Blocks whose side properties are their connections to the neighbours.
fn connects(name: &str) -> bool {
    name.ends_with("_fence")
        || name.ends_with("_pane")
        || (name.ends_with("_wall") && !name.contains("_wall_"))
        || matches!(name, "minecraft:iron_bars" | "minecraft:redstone_wire" | "minecraft:tripwire")
}

/// Properties placing works out from the neighbours (and old formats don't store).
fn follows_neighbours(name: &str, key: &str) -> bool {
    (connects(name) && matches!(key, "north" | "east" | "south" | "west" | "up"))
        || (key == "shape" && name.ends_with("_stairs"))
}

/// Compares the world block `actual` with the schematic's `wanted` (`None`: outside the
/// schematic, nothing to say). `actual_replaceable`: grass, snow, fluids — a missing block, not
/// an extra one. `strict` compares every property.
pub fn classify(
    wanted: Option<&BlockState>,
    actual: &BlockState,
    actual_replaceable: bool,
    strict: bool,
) -> Option<Status> {
    classify_power(wanted, actual, actual_replaceable, strict, false)
}

/// [`classify`], with redstone's on/off (`powered`, `lit`, `power`...) counted when `power`.
pub fn classify_power(
    wanted: Option<&BlockState>,
    actual: &BlockState,
    actual_replaceable: bool,
    strict: bool,
    power: bool,
) -> Option<Status> {
    let wanted = wanted?;
    let actual_empty = actual.is_air() || actual_replaceable;
    Some(match (wanted.is_air(), actual_empty) {
        (true, true) => return None,
        (true, false) => Status::Extra,
        (false, true) => {
            if !actual.is_air() && same_block(wanted, actual) {
                // A replaceable block that is the wanted one (snow layers, grass).
                state_status(wanted, actual, strict, power)
            } else {
                Status::Missing
            }
        }
        (false, false) => {
            if same_block(wanted, actual) {
                state_status(wanted, actual, strict, power)
            } else {
                Status::WrongBlock
            }
        }
    })
}

/// Whether two names are one block, or one plant at another stage of growing: a placed bamboo
/// shoot becomes a stalk, and a kelp or vine's tip becomes body when the next one goes on.
fn same_block(a: &BlockState, b: &BlockState) -> bool {
    a.name == b.name || plant_family(&a.name) == plant_family(&b.name)
}

fn plant_family(name: &str) -> &str {
    match name {
        "minecraft:bamboo_sapling" => "minecraft:bamboo",
        "minecraft:kelp_plant" => "minecraft:kelp",
        "minecraft:twisting_vines_plant" => "minecraft:twisting_vines",
        "minecraft:weeping_vines_plant" => "minecraft:weeping_vines",
        "minecraft:cave_vines_plant" => "minecraft:cave_vines",
        "minecraft:big_dripleaf_stem" => "minecraft:big_dripleaf",
        "minecraft:attached_pumpkin_stem" => "minecraft:pumpkin_stem",
        "minecraft:attached_melon_stem" => "minecraft:melon_stem",
        _ => name,
    }
}

fn state_status(wanted: &BlockState, actual: &BlockState, strict: bool, power: bool) -> Status {
    for (k, v) in &wanted.properties {
        if !strict
            && (IGNORED_PROPS.contains(&k.as_str())
                || (!power && POWER_PROPS.contains(&k.as_str()))
                || follows_neighbours(&wanted.name, k))
        {
            continue;
        }
        if let Some(a) = actual.properties.get(k)
            && a != v
        {
            return Status::WrongState;
        }
    }
    Status::Correct
}

/// Whether placing gave a block the printer can leave (or finish): the wanted block, with
/// anything that comes later or doesn't count left out: redstone's on/off, the settings clicked
/// after placing, connections, stair shapes, and a chest still waiting for its other half.
pub fn placement_ok(wanted: &BlockState, got: &BlockState) -> bool {
    if !same_block(wanted, got) {
        return false;
    }
    for (k, v) in &wanted.properties {
        let Some(g) = got.properties.get(k) else { continue };
        if g == v
            || IGNORED_PROPS.contains(&k.as_str())
            || POWER_PROPS.contains(&k.as_str())
            || CONFIG_PROPS.contains(&k.as_str())
            || follows_neighbours(&wanted.name, k)
        {
            continue;
        }
        // A chest placed alone joins its other half when that one is placed.
        if k == "type" && is_chest(&wanted.name) && g == "single" {
            continue;
        }
        return false;
    }
    true
}

fn is_chest(name: &str) -> bool {
    matches!(name, "minecraft:chest" | "minecraft:trapped_chest") || name.ends_with("copper_chest")
}

/// How many clicks set a placed block to the wanted setting (`CONFIG_PROPS`), when that is all
/// that is wrong with it.
pub fn config_clicks(wanted: &BlockState, actual: &BlockState) -> Option<u32> {
    if wanted.name != actual.name {
        return None;
    }
    let mut clicks = None;
    for (k, v) in &wanted.properties {
        let Some(a) = actual.properties.get(k) else { continue };
        if a == v
            || IGNORED_PROPS.contains(&k.as_str())
            || POWER_PROPS.contains(&k.as_str())
            || follows_neighbours(&wanted.name, k)
        {
            continue;
        }
        let n = match k.as_str() {
            // Repeater: 1 -> 2 -> 3 -> 4 -> 1.
            "delay" => cycle(a, v, 4, 1)?,
            // Note block: 0..24, +1 a click.
            "note" => cycle(a, v, 25, 0)?,
            // Comparator compare/subtract, daylight detector inverted: one click flips.
            "mode" | "inverted" => 1,
            _ => return None,
        };
        if clicks.is_some() {
            return None;
        }
        clicks = Some(n);
    }
    clicks.filter(|&n| n > 0)
}

fn cycle(actual: &str, wanted: &str, n: u32, first: u32) -> Option<u32> {
    let a = actual.parse::<u32>().ok()?.checked_sub(first)?;
    let w = wanted.parse::<u32>().ok()?.checked_sub(first)?;
    Some((w + n - a % n) % n)
}

/// Whether clicking the block that is there with another of its item makes more of it: half a
/// slab to a double slab, more candles, sea pickles, snow layers, turtle eggs, petals.
pub fn upgradable(wanted: &BlockState, actual: &BlockState) -> bool {
    if wanted.name != actual.name {
        return false;
    }
    if wanted.name.ends_with("_slab") {
        return wanted.prop("type") == Some("double") && matches!(actual.prop("type"), Some("bottom" | "top"));
    }
    ["pickles", "candles", "layers", "eggs", "flower_amount", "segment_amount"].iter().any(|k| {
        let n = |s: &BlockState| s.prop(k).and_then(|v| v.parse::<u32>().ok());
        matches!((n(wanted), n(actual)), (Some(w), Some(a)) if w > a)
    })
}

/// Counts by status, for the HUD.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub correct: u64,
    pub missing: u64,
    pub extra: u64,
    pub wrong_block: u64,
    pub wrong_state: u64,
}

impl Tally {
    pub fn add(&mut self, s: Status) {
        match s {
            Status::Correct => self.correct += 1,
            Status::Missing => self.missing += 1,
            Status::Extra => self.extra += 1,
            Status::WrongBlock => self.wrong_block += 1,
            Status::WrongState => self.wrong_state += 1,
        }
    }
    pub fn wanted(&self) -> u64 {
        self.correct + self.missing + self.wrong_block + self.wrong_state
    }
    pub fn percent(&self) -> f64 {
        let w = self.wanted();
        if w == 0 { 100.0 } else { self.correct as f64 * 100.0 / w as f64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        let stone = BlockState::new("minecraft:stone", &[]);
        let air = BlockState::air();
        let n = BlockState::new("minecraft:furnace", &[("facing", "north"), ("lit", "false")]);
        let s = BlockState::new("minecraft:furnace", &[("facing", "south"), ("lit", "false")]);
        let lit = BlockState::new("minecraft:furnace", &[("facing", "north"), ("lit", "true")]);
        let grass = BlockState::new("minecraft:short_grass", &[]);
        assert_eq!(classify(Some(&stone), &air, false, false), Some(Status::Missing));
        assert_eq!(classify(Some(&stone), &grass, true, false), Some(Status::Missing));
        assert_eq!(classify(Some(&air), &stone, false, false), Some(Status::Extra));
        assert_eq!(classify(Some(&air), &grass, true, false), None);
        assert_eq!(classify(Some(&stone), &n, false, false), Some(Status::WrongBlock));
        assert_eq!(classify(Some(&n), &s, false, false), Some(Status::WrongState));
        assert_eq!(classify(Some(&n), &lit, false, false), Some(Status::Correct));
        assert_eq!(classify(Some(&n), &lit, false, true), Some(Status::WrongState));
        assert_eq!(classify(None, &stone, false, false), None);
    }

    #[test]
    fn connections_follow_neighbours() {
        let want = BlockState::new("minecraft:oak_fence", &[("north", "false"), ("east", "true")]);
        let got = BlockState::new("minecraft:oak_fence", &[("north", "true"), ("east", "false")]);
        assert_eq!(classify(Some(&want), &got, false, false), Some(Status::Correct));
        assert_eq!(classify(Some(&want), &got, false, true), Some(Status::WrongState));
    }

    fn st(name: &str, props: &[(&str, &str)]) -> BlockState {
        BlockState::new(name, props)
    }

    #[test]
    fn placing_leaves_power_settings_and_chest_halves() {
        let want = st("minecraft:repeater", &[("delay", "3"), ("facing", "north"), ("powered", "true")]);
        assert!(placement_ok(&want, &st("minecraft:repeater", &[("delay", "1"), ("facing", "north"), ("powered", "false")])));
        assert!(!placement_ok(&want, &st("minecraft:repeater", &[("delay", "3"), ("facing", "south"), ("powered", "true")])));
        let chest = st("minecraft:chest", &[("facing", "east"), ("type", "left")]);
        assert!(placement_ok(&chest, &st("minecraft:chest", &[("facing", "east"), ("type", "single")])));
        assert!(!placement_ok(&chest, &st("minecraft:chest", &[("facing", "east"), ("type", "right")])), "joined the wrong chest");
        let single = st("minecraft:chest", &[("facing", "east"), ("type", "single")]);
        assert!(!placement_ok(&single, &st("minecraft:chest", &[("facing", "east"), ("type", "left")])), "must stay single");
        let slab = st("minecraft:oak_slab", &[("type", "double")]);
        assert!(!placement_ok(&slab, &st("minecraft:oak_slab", &[("type", "bottom")])), "a half isn't a double");
    }

    #[test]
    fn plants_at_any_stage() {
        let bamboo = |age: &str, leaves: &str, stage: &str| st("minecraft:bamboo", &[("age", age), ("leaves", leaves), ("stage", stage)]);
        let shoot = st("minecraft:bamboo_sapling", &[]);
        for wanted in [bamboo("1", "large", "0"), bamboo("0", "none", "1"), bamboo("1", "small", "1")] {
            assert!(placement_ok(&wanted, &shoot), "a click on the ground gives a shoot");
            assert!(placement_ok(&wanted, &bamboo("0", "none", "0")));
            assert_eq!(classify(Some(&wanted), &shoot, false, false), Some(Status::Correct));
            assert_eq!(classify(Some(&wanted), &bamboo("0", "none", "0"), false, false), Some(Status::Correct));
        }
        assert_eq!(classify(Some(&shoot), &bamboo("1", "large", "0"), false, false), Some(Status::Correct), "it grew");
        let body = st("minecraft:kelp_plant", &[]);
        let tip = st("minecraft:kelp", &[("age", "3")]);
        assert!(placement_ok(&body, &tip));
        assert!(!placement_ok(&body, &shoot));
        assert_eq!(classify(Some(&body), &st("minecraft:stone", &[]), false, false), Some(Status::WrongBlock));
    }

    #[test]
    fn clicks_to_set_up() {
        let r = |d: &str| st("minecraft:repeater", &[("delay", d), ("facing", "north"), ("powered", "false")]);
        assert_eq!(config_clicks(&r("3"), &r("1")), Some(2));
        assert_eq!(config_clicks(&r("1"), &r("4")), Some(1), "4 wraps to 1");
        assert_eq!(config_clicks(&r("2"), &r("2")), None);
        let c = |m: &str| st("minecraft:comparator", &[("mode", m), ("facing", "west")]);
        assert_eq!(config_clicks(&c("subtract"), &c("compare")), Some(1));
        let n = |v: &str| st("minecraft:note_block", &[("note", v), ("instrument", "harp")]);
        assert_eq!(config_clicks(&n("1"), &n("24")), Some(2));
        let wrong_facing = st("minecraft:repeater", &[("delay", "1"), ("facing", "south"), ("powered", "false")]);
        assert_eq!(config_clicks(&r("3"), &wrong_facing), None, "turned the wrong way: clicking can't fix it");
    }

    #[test]
    fn stacking_more_of_a_block() {
        assert!(upgradable(&st("minecraft:oak_slab", &[("type", "double")]), &st("minecraft:oak_slab", &[("type", "bottom")])));
        assert!(!upgradable(&st("minecraft:oak_slab", &[("type", "top")]), &st("minecraft:oak_slab", &[("type", "bottom")])));
        assert!(upgradable(&st("minecraft:candle", &[("candles", "3")]), &st("minecraft:candle", &[("candles", "1")])));
        assert!(!upgradable(&st("minecraft:candle", &[("candles", "1")]), &st("minecraft:candle", &[("candles", "3")])));
    }

    #[test]
    fn power_counts_when_asked() {
        let want = st("minecraft:lever", &[("powered", "true"), ("face", "floor"), ("facing", "north")]);
        let got = st("minecraft:lever", &[("powered", "false"), ("face", "floor"), ("facing", "north")]);
        assert_eq!(classify_power(Some(&want), &got, false, false, false), Some(Status::Correct));
        assert_eq!(classify_power(Some(&want), &got, false, false, true), Some(Status::WrongState));
    }
}
