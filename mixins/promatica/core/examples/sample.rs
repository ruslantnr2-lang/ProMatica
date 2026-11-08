//! Writes a small test schematic: `cargo run -p promatica-core --example sample -- house.litematic`
//! A 7x5x7 stone-brick hut with oak stairs, a door, a furnace, a chest and a lantern: directional
//! blocks to check the overlay, the verifier and the printer's facing.

use promatica_core::schematic::{Metadata, Region};
use promatica_core::{BlockState, Schematic};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "sample_hut.litematic".into());
    let s = |n: &str, p: &[(&str, &str)]| BlockState::new(n, p);
    let region = Region::from_fn("hut", [0, 0, 0], [7, 5, 7], |[x, y, z]| {
        let edge = x == 0 || x == 6 || z == 0 || z == 6;
        match y {
            0 => s("minecraft:stone_bricks", &[]),
            1 | 2 if edge => {
                if z == 0 && x == 3 {
                    let half = if y == 1 { "lower" } else { "upper" };
                    s("minecraft:oak_door", &[("facing", "north"), ("half", half), ("hinge", "left"), ("open", "false"), ("powered", "false")])
                } else if (x == 0 || x == 6) && z == 3 && y == 2 {
                    s("minecraft:glass_pane", &[("north", "true"), ("south", "true"), ("east", "false"), ("west", "false"), ("waterlogged", "false")])
                } else {
                    s("minecraft:oak_planks", &[])
                }
            }
            1 if x == 1 && z == 5 => s("minecraft:furnace", &[("facing", "south"), ("lit", "false")]),
            1 if x == 5 && z == 5 => s("minecraft:chest", &[("facing", "north"), ("type", "single"), ("waterlogged", "false")]),
            3 if edge => {
                let facing = if z == 0 { "south" } else if z == 6 { "north" } else if x == 0 { "east" } else { "west" };
                s("minecraft:oak_stairs", &[("facing", facing), ("half", "bottom"), ("shape", "straight"), ("waterlogged", "false")])
            }
            3 => s("minecraft:oak_slab", &[("type", "bottom"), ("waterlogged", "false")]),
            4 if x == 3 && z == 3 => s("minecraft:lantern", &[("hanging", "false"), ("waterlogged", "false")]),
            _ => BlockState::air(),
        }
    });
    let mut schematic = Schematic {
        version: 6,
        data_version: 4671,
        metadata: Metadata {
            name: "Sample hut".into(),
            author: "promatica".into(),
            description: "Test schematic".into(),
            region_count: 1,
            total_volume: 7 * 5 * 7,
            enclosing_size: [7, 5, 7],
            ..Default::default()
        },
        regions: vec![region],
    };
    schematic.metadata.total_blocks = schematic.block_count() as i64;
    std::fs::write(&out, schematic.to_bytes()).expect("write");
    println!("wrote {out} ({} blocks)", schematic.metadata.total_blocks);
}
