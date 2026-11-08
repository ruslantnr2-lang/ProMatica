//! The material list: how many of which item a schematic needs (`MaterialListUtils`).

use crate::schematic::BlockState;
use std::collections::BTreeMap;

/// How many items of the block's item one cell of `state` takes (`MaterialCache` and
/// `overrideStackSize`): 0 for parts placed with another (a door's top, a bed's head), the
/// count of slabs, snow layers, candles, pickles, eggs, petals and multiface faces.
pub fn items_for_state(state: &BlockState) -> u32 {
    if state.is_air() || state.is_void() {
        return 0;
    }
    let name = state.name.as_str();
    let p = |k: &str| state.prop(k);
    let num = |k: &str| p(k).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
    match name {
        "minecraft:piston_head" | "minecraft:moving_piston" | "minecraft:nether_portal" | "minecraft:end_portal" | "minecraft:end_gateway" | "minecraft:fire" | "minecraft:soul_fire" | "minecraft:bubble_column" | "minecraft:frosted_ice" => return 0,
        "minecraft:snow" => return num("layers"),
        "minecraft:sea_pickle" => return num("pickles"),
        "minecraft:turtle_egg" | "minecraft:sniffer_egg" => return num("eggs"),
        "minecraft:pink_petals" | "minecraft:wildflowers" => return num("flower_amount"),
        "minecraft:leaf_litter" => return num("segment_amount"),
        _ => {}
    }
    if name.ends_with("candle") && p("candles").is_some() {
        return num("candles");
    }
    if p("type") == Some("double") && name.ends_with("_slab") {
        return 2;
    }
    if p("half") == Some("upper") {
        // Doors, tall flowers, tall grass, small dripleaf: the lower half places both
        // (stairs and trapdoors use top/bottom, not upper/lower).
        return 0;
    }
    if p("part") == Some("head") && name.ends_with("_bed") {
        return 0;
    }
    if matches!(name, "minecraft:glow_lichen" | "minecraft:sculk_vein" | "minecraft:resin_clump") {
        let faces = ["north", "east", "south", "west", "up", "down"]
            .iter()
            .filter(|f| p(f) == Some("true"))
            .count() as u32;
        return faces.max(1);
    }
    1
}

/// The item of a block with none of its own, which is what placed it (`getCloneItemStack`).
pub fn planted_by(block: &str) -> Option<&'static str> {
    Some(match block {
        "minecraft:pumpkin_stem" | "minecraft:attached_pumpkin_stem" => "minecraft:pumpkin_seeds",
        "minecraft:melon_stem" | "minecraft:attached_melon_stem" => "minecraft:melon_seeds",
        "minecraft:wheat" => "minecraft:wheat_seeds",
        "minecraft:carrots" => "minecraft:carrot",
        "minecraft:potatoes" => "minecraft:potato",
        "minecraft:beetroots" => "minecraft:beetroot_seeds",
        "minecraft:torchflower_crop" => "minecraft:torchflower_seeds",
        "minecraft:bamboo_sapling" => "minecraft:bamboo",
        "minecraft:sweet_berry_bush" => "minecraft:sweet_berries",
        "minecraft:cave_vines" | "minecraft:cave_vines_plant" => "minecraft:glow_berries",
        "minecraft:big_dripleaf_stem" => "minecraft:big_dripleaf",
        "minecraft:kelp_plant" => "minecraft:kelp",
        "minecraft:twisting_vines_plant" => "minecraft:twisting_vines",
        "minecraft:weeping_vines_plant" => "minecraft:weeping_vines",
        _ => return None,
    })
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Material {
    pub item: String,
    pub total: u64,
    pub missing: u64,
    pub available: u64,
}

impl Material {
    /// Still to fetch: what's missing in the world beyond what the player carries.
    pub fn to_fetch(&self) -> u64 {
        self.missing.saturating_sub(self.available)
    }
}

/// Builds the list from (item, count total, count missing) triples and the inventory's counts.
pub fn build_list(
    totals: &BTreeMap<String, u64>,
    missing: &BTreeMap<String, u64>,
    inventory: &BTreeMap<String, u64>,
) -> Vec<Material> {
    let mut list: Vec<Material> = totals
        .iter()
        .map(|(item, &total)| Material {
            item: item.clone(),
            total,
            missing: missing.get(item).copied().unwrap_or(0),
            available: inventory.get(item).copied().unwrap_or(0),
        })
        .collect();
    // Most missing first, then most needed, then by name: what to fetch on top.
    list.sort_by(|a, b| {
        b.to_fetch()
            .cmp(&a.to_fetch())
            .then(b.missing.cmp(&a.missing))
            .then(b.total.cmp(&a.total))
            .then(a.item.cmp(&b.item))
    });
    list
}

/// `64 x 3 + 5` style counts, as Litematica's list shows them.
pub fn stacks(count: u64, stack: u64) -> String {
    let stack = stack.max(1);
    if count < stack {
        return count.to_string();
    }
    let (s, r) = (count / stack, count % stack);
    if r == 0 { format!("{count} ({s}x{stack})") } else { format!("{count} ({s}x{stack}+{r})") }
}

/// The list as a table for a spreadsheet: tab-separated with a header row, which Excel and others
/// split into columns when pasted. `name` gives an item's readable name.
pub fn table(list: &[Material], name: impl Fn(&str) -> String) -> String {
    let mut out = String::from("Item\tMissing\tTotal\tCarried\tTo fetch\tItem ID\r\n");
    for m in list {
        out += &format!("{}\t{}\t{}\t{}\t{}\t{}\r\n", name(&m.item), m.missing, m.total, m.available, m.to_fetch(), m.item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_counts() {
        let st = |n: &str, p: &[(&str, &str)]| BlockState::new(n, p);
        assert_eq!(items_for_state(&st("minecraft:stone", &[])), 1);
        assert_eq!(items_for_state(&st("minecraft:oak_slab", &[("type", "double")])), 2);
        assert_eq!(items_for_state(&st("minecraft:oak_slab", &[("type", "top")])), 1);
        assert_eq!(items_for_state(&st("minecraft:snow", &[("layers", "5")])), 5);
        assert_eq!(items_for_state(&st("minecraft:oak_door", &[("half", "upper"), ("facing", "north")])), 0);
        assert_eq!(items_for_state(&st("minecraft:oak_door", &[("half", "lower"), ("facing", "north")])), 1);
        assert_eq!(items_for_state(&st("minecraft:tall_grass", &[("half", "upper")])), 0);
        assert_eq!(items_for_state(&st("minecraft:oak_stairs", &[("half", "top"), ("facing", "north")])), 1);
        assert_eq!(items_for_state(&st("minecraft:red_bed", &[("part", "head")])), 0);
        assert_eq!(items_for_state(&st("minecraft:white_candle", &[("candles", "3")])), 3);
        assert_eq!(items_for_state(&st("minecraft:glow_lichen", &[("north", "true"), ("up", "true"), ("south", "false")])), 2);
        assert_eq!(items_for_state(&BlockState::air()), 0);
    }

    #[test]
    fn list_order_and_stacks() {
        let totals = BTreeMap::from([("a".to_string(), 10), ("b".to_string(), 100)]);
        let missing = BTreeMap::from([("a".to_string(), 10), ("b".to_string(), 5)]);
        let inv = BTreeMap::from([("b".to_string(), 64)]);
        let list = build_list(&totals, &missing, &inv);
        assert_eq!(list[0].item, "a");
        assert_eq!(list[1].to_fetch(), 0);
        assert_eq!(stacks(200, 64), "200 (3x64+8)");
        assert_eq!(stacks(12, 64), "12");
    }

    #[test]
    fn table_has_a_header_and_a_row_per_item() {
        let list = [Material { item: "minecraft:stone".into(), total: 10, missing: 7, available: 3 }];
        let text = table(&list, |_| "Stone".to_string());
        assert_eq!(text, "Item\tMissing\tTotal\tCarried\tTo fetch\tItem ID\r\nStone\t7\t10\t3\t4\tminecraft:stone\r\n");
    }
}
