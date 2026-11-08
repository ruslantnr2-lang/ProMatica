//! The schematic formats besides `.litematic`, read into the same [`Schematic`]:
//!
//! - Sponge `.schem` v1–v3 (WorldEdit, FAWE, Amulet, Axiom): a palette of state strings and
//!   varint indices.
//! - Vanilla structure `.nbt` (structure blocks, datapacks): a palette and a list of blocks;
//!   cells not listed are `structure_void` (left alone).
//! - MCEdit / old WorldEdit `.schematic`: 1.12 numeric ids and data values, mapped to 1.13 states
//!   with vanilla's flattening table, plus what the old format kept elsewhere (door halves, tall
//!   plants, skull and bed types, banner colours, double chests, snowy grass).
//!
//! The states come out in the file's own Minecraft version ([`Schematic::data_version`]); the
//! game side brings them up to date with Horizon's data fixer.

use crate::nbt::{self, Tag};
use crate::bitarray::BitArray;
use crate::schematic::{BlockState, MAX_UNPACKED, MAX_VOLUME, Metadata, Region, Schematic, read_state, vec3};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Litematic,
    Sponge(u8),
    Structure,
    McEdit,
}

impl Format {
    pub fn name(self) -> String {
        match self {
            Format::Litematic => "Litematica".into(),
            Format::Sponge(v) => format!("Sponge v{v} (.schem)"),
            Format::Structure => "structure (.nbt)".into(),
            Format::McEdit => "MCEdit (.schematic, 1.12)".into(),
        }
    }
}

/// File endings the schematics folder is listed for.
pub const EXTENSIONS: &[&str] = &[".litematic", ".schem", ".schematic", ".nbt"];

/// The version the MCEdit table's states belong to: right at the flattening (1.13).
pub const FLATTENED_DATA_VERSION: i64 = 1450;
/// Sponge v1 has no DataVersion; it was written by WorldEdit for 1.13.
const SPONGE_V1_DATA_VERSION: i64 = 1519;
/// Structure files from before 1.9 had none either (their names are 1.12's).
const OLD_STRUCTURE_DATA_VERSION: i64 = 1343;

/// Reads any supported file, telling the format by its contents (not its name).
pub fn load(bytes: &[u8]) -> Result<(Schematic, Format), String> {
    let root = nbt::read_file_bytes(bytes, MAX_UNPACKED)?;
    let format = detect(&root).ok_or("not a schematic (unknown format)")?;
    let schematic = match format {
        Format::Litematic => Schematic::from_nbt(root)?,
        Format::Sponge(v) => sponge(sponge_root(&root), v)?,
        Format::Structure => structure(&root)?,
        Format::McEdit => mcedit(&root)?,
    };
    Ok((schematic, format))
}

fn sponge_root(root: &Tag) -> &Tag {
    root.get("Schematic").filter(|t| t.compound().is_some()).unwrap_or(root)
}

/// What a file is, without keeping its blocks (shown before loading it).
#[derive(Clone, Debug)]
pub struct Summary {
    pub name: String,
    pub author: String,
    pub format: Format,
    pub size: [i32; 3],
    pub blocks: u64,
    pub regions: usize,
}

/// Reads what a file is. A `.litematic` tells it in its metadata, so its block arrays are
/// skipped; other files (and a `.litematic` without a block count) are read in full.
pub fn summary(bytes: &[u8]) -> Result<Summary, String> {
    let light = nbt::read_file_bytes_light(bytes, MAX_UNPACKED)?;
    if detect(&light) == Some(Format::Litematic) {
        let m = light.get("Metadata");
        let text = |k: &str| m.and_then(|m| m.get(k)).and_then(Tag::str).unwrap_or("").to_string();
        let blocks = m.and_then(|m| m.get("TotalBlocks")).and_then(Tag::int).unwrap_or(0);
        let regions = light.get("Regions").and_then(Tag::compound);
        if blocks > 0
            && let Some(regions) = regions
        {
            let mut lo = [i32::MAX; 3];
            let mut hi = [i32::MIN; 3];
            for r in regions.values() {
                let (Some(p), Some(s)) = (vec3(r.get("Position")), vec3(r.get("Size"))) else { continue };
                for i in 0..3 {
                    let end = p[i] + if s[i] >= 0 { s[i] - 1 } else { s[i] + 1 };
                    lo[i] = lo[i].min(p[i].min(end));
                    hi[i] = hi[i].max(p[i].max(end));
                }
            }
            if lo[0] <= hi[0] {
                return Ok(Summary {
                    name: text("Name"),
                    author: text("Author"),
                    format: Format::Litematic,
                    size: [0, 1, 2].map(|i| hi[i] - lo[i] + 1),
                    blocks: blocks as u64,
                    regions: regions.len(),
                });
            }
        }
    }
    drop(light);
    let (s, format) = load(bytes)?;
    let (lo, hi) = s.bounds();
    Ok(Summary {
        name: s.metadata.name.clone(),
        author: s.metadata.author.clone(),
        format,
        size: [0, 1, 2].map(|i| hi[i] - lo[i] + 1),
        blocks: if s.metadata.total_blocks > 0 { s.metadata.total_blocks as u64 } else { s.block_count() },
        regions: s.regions.len(),
    })
}

pub fn detect(root: &Tag) -> Option<Format> {
    if root.get("Regions").is_some() {
        return Some(Format::Litematic);
    }
    let s = sponge_root(root);
    if s.get("Blocks").and_then(Tag::compound).is_some() && s.get("Width").is_some() {
        return Some(Format::Sponge(3));
    }
    if s.get("BlockData").is_some() && s.get("Palette").is_some() {
        let v = s.get("Version").and_then(Tag::int).unwrap_or(1);
        return Some(Format::Sponge(v.clamp(1, 2) as u8));
    }
    if matches!(root.get("Blocks"), Some(Tag::ByteArray(_))) {
        return Some(Format::McEdit);
    }
    if root.get("blocks").is_some() && root.get("size").is_some() {
        return Some(Format::Structure);
    }
    None
}

// ---------------------------------------------------------------- shared

fn dims(t: &Tag) -> Result<[i32; 3], String> {
    // Shorts in the file, but unsigned.
    let d = |k: &str| {
        t.get(k)
            .and_then(Tag::int)
            .map(|v| if v < 0 { v + 65536 } else { v } as i32)
            .ok_or_else(|| format!("no {k}"))
    };
    let size = [d("Width")?, d("Height")?, d("Length")?];
    check_volume(size)?;
    Ok(size)
}

fn check_volume(size: [i32; 3]) -> Result<u64, String> {
    if size.iter().any(|&v| v <= 0) {
        return Err(format!("empty size {size:?}"));
    }
    let volume: u64 = size.iter().map(|&v| v as u64).product();
    if volume > MAX_VOLUME {
        return Err(format!("too large ({volume} blocks)"));
    }
    Ok(volume)
}

fn ints(t: Option<&Tag>) -> Option<Vec<i64>> {
    match t? {
        Tag::IntArray(v) => Some(v.iter().map(|&i| i.into()).collect()),
        Tag::List(items) => items.iter().map(Tag::int).collect(),
        _ => None,
    }
}

fn pos3(t: Option<&Tag>) -> Option<[i32; 3]> {
    let v = ints(t)?;
    (v.len() == 3).then(|| [v[0] as i32, v[1] as i32, v[2] as i32])
}

/// Litematica's cell order: x fastest, then z, then y (Sponge and MCEdit use the same).
fn cell(size: [i32; 3], p: [i32; 3]) -> usize {
    ((p[1] * size[2] + p[2]) * size[0] + p[0]) as usize
}

fn finish(name: String, author: String, data_version: i64, region: Region) -> Schematic {
    let total_blocks = region.block_count() as i64;
    let metadata = Metadata {
        name,
        author,
        region_count: 1,
        total_volume: region.volume() as i64,
        total_blocks,
        enclosing_size: region.size,
        ..Metadata::default()
    };
    Schematic { version: 0, data_version, metadata, regions: vec![region] }
}

/// A palette built cell by cell, with air at 0.
struct Palette {
    states: Vec<BlockState>,
    index: HashMap<BlockState, u32>,
}

impl Palette {
    fn new() -> Self {
        let air = BlockState::air();
        Self { states: vec![air.clone()], index: HashMap::from([(air, 0)]) }
    }
    fn id(&mut self, s: BlockState) -> u32 {
        if s.is_air() {
            return 0;
        }
        if let Some(&i) = self.index.get(&s) {
            return i;
        }
        let i = self.states.len() as u32;
        self.states.push(s.clone());
        self.index.insert(s, i);
        i
    }
}

// ---------------------------------------------------------------- Sponge

fn sponge(s: &Tag, version: u8) -> Result<Schematic, String> {
    let size = dims(s)?;
    let volume = check_volume(size)? as usize;
    let (palette_tag, data_tag, entities_tag) = if version >= 3 {
        let b = s.get("Blocks").ok_or("no Blocks")?;
        (b.get("Palette"), b.get("Data"), b.get("BlockEntities"))
    } else {
        (s.get("Palette"), s.get("BlockData"), s.get("BlockEntities").or_else(|| s.get("TileEntities")))
    };
    let file_palette = palette_tag.and_then(Tag::compound).ok_or("no Palette")?;
    let mut palette = Palette::new();
    let mut remap: HashMap<i64, u32> = HashMap::new();
    for (text, idx) in file_palette {
        let idx = idx.int().ok_or("palette index is not a number")?;
        let state = BlockState::parse(text).ok_or_else(|| format!("bad palette entry {text}"))?;
        remap.insert(idx, palette.id(state));
    }
    let Some(Tag::ByteArray(data)) = data_tag else { return Err("no block data".into()) };
    // The palette is known up front: cells go straight into an array of its width.
    let mut blocks = BitArray::new(BitArray::bits_for(palette.states.len()), volume as u64);
    let mut bytes = data.iter().map(|&b| b as u8);
    for i in 0..volume as u64 {
        // Unsigned LEB128 varints.
        let mut value = 0i64;
        let mut shift = 0;
        loop {
            let b = bytes.next().ok_or("block data ends early")?;
            value |= i64::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 35 {
                return Err("bad varint in block data".into());
            }
        }
        let id = *remap.get(&value).ok_or_else(|| format!("block data names palette entry {value}"))?;
        if id != 0 {
            blocks.set(i, id);
        }
    }
    let block_entities = entities_tag
        .and_then(Tag::list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|t| Some((pos3(t.get("Pos"))?, t.clone())))
        .collect();
    let meta = s.get("Metadata");
    let text = |k: &str| meta.and_then(|m| m.get(k)).and_then(Tag::str).unwrap_or("").to_string();
    let data_version = s.get("DataVersion").and_then(Tag::int).unwrap_or(SPONGE_V1_DATA_VERSION);
    let region = Region::from_bits("Main", size, palette.states, blocks, block_entities);
    Ok(finish(text("Name"), text("Author"), data_version, region))
}

// ---------------------------------------------------------------- vanilla structure

fn structure(root: &Tag) -> Result<Schematic, String> {
    let size = pos3(root.get("size")).ok_or("no size")?;
    let volume = check_volume(size)? as usize;
    let file_palette = match root.get("palette").and_then(Tag::list) {
        Some(p) => p,
        // Shipwrecks and the like keep several palettes to pick from: the first one.
        None => root
            .get("palettes")
            .and_then(Tag::list)
            .and_then(|p| p.first())
            .and_then(Tag::list)
            .ok_or("no palette")?,
    };
    let mut palette = Palette::new();
    let remap: Vec<u32> =
        file_palette.iter().map(|t| read_state(t).map(|s| palette.id(s))).collect::<Result<_, _>>()?;
    // Cells the file doesn't list are left alone when placed.
    let void = palette.id(BlockState::new("minecraft:structure_void", &[]));
    let mut indices = vec![void; volume];
    let mut block_entities = Vec::new();
    for b in root.get("blocks").and_then(Tag::list).unwrap_or(&[]) {
        let Some(p) = pos3(b.get("pos")) else { continue };
        if (0..3).any(|i| p[i] < 0 || p[i] >= size[i]) {
            continue;
        }
        let Some(&id) = b.get("state").and_then(Tag::int).and_then(|i| remap.get(i as usize)) else { continue };
        indices[cell(size, p)] = id;
        if let Some(nbt) = b.get("nbt") {
            block_entities.push((p, nbt.clone()));
        }
    }
    let data_version = root.get("DataVersion").and_then(Tag::int).unwrap_or(OLD_STRUCTURE_DATA_VERSION);
    let author = root.get("author").and_then(Tag::str).unwrap_or("").to_string();
    let region = Region::from_indices("Main", size, palette.states, &indices, block_entities);
    Ok(finish(String::new(), author, data_version, region))
}

// ---------------------------------------------------------------- MCEdit

struct Legacy {
    exact: HashMap<u32, (String, BTreeMap<String, String>)>,
    /// A block id's first registered state: ids without their own line use it.
    first: HashMap<u32, u32>,
}

fn legacy() -> &'static Legacy {
    static TABLE: OnceLock<Legacy> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut exact = HashMap::new();
        let mut first = HashMap::new();
        for line in crate::legacy_blocks::TABLE.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut f = line.split('\t');
            let (Some(id), Some(name)) = (f.next().and_then(|v| v.parse::<u32>().ok()), f.next()) else { continue };
            let props = f
                .next()
                .unwrap_or("")
                .split(';')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            first.entry(id >> 4).or_insert(id);
            exact.insert(id, (name.to_string(), props));
        }
        Legacy { exact, first }
    })
}

/// The 1.13 state of a 1.12 `blockId << 4 | data`.
pub fn legacy_state(id: u32) -> BlockState {
    let t = legacy();
    match t.exact.get(&id).or_else(|| t.first.get(&(id >> 4)).and_then(|f| t.exact.get(f))) {
        Some((name, properties)) => BlockState { name: name.clone(), properties: properties.clone() },
        None => BlockState::air(),
    }
}

const COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan",
    "purple", "blue", "brown", "green", "red", "black",
];

fn recolor(name: &str, color: &str) -> String {
    let rest = name.strip_prefix("minecraft:").unwrap_or(name);
    let rest = COLORS
        .iter()
        .filter(|c| rest.starts_with(&format!("{c}_")))
        .max_by_key(|c| c.len())
        .map_or(rest, |c| &rest[c.len() + 1..]);
    format!("minecraft:{color}_{rest}")
}

/// The skull from the old skull block (its kind and rotation were in the block entity).
fn skull(state: &BlockState, be: Option<&Tag>) -> BlockState {
    const KINDS: [(&str, &str); 6] = [
        ("skeleton_skull", "skeleton_wall_skull"),
        ("wither_skeleton_skull", "wither_skeleton_wall_skull"),
        ("zombie_head", "zombie_wall_head"),
        ("player_head", "player_wall_head"),
        ("creeper_head", "creeper_wall_head"),
        ("dragon_head", "dragon_wall_head"),
    ];
    let kind = be.and_then(|t| t.get("SkullType")).and_then(Tag::int).unwrap_or(0).clamp(0, 5) as usize;
    match state.prop("facing") {
        Some(f @ ("north" | "south" | "west" | "east")) => {
            BlockState::new(&format!("minecraft:{}", KINDS[kind].1), &[("facing", f)])
        }
        _ => {
            let rot = be.and_then(|t| t.get("Rot")).and_then(Tag::int).unwrap_or(0).rem_euclid(16).to_string();
            BlockState::new(&format!("minecraft:{}", KINDS[kind].0), &[("rotation", &rot)])
        }
    }
}

const TALL_PLANTS: [&str; 6] = [
    "minecraft:sunflower",
    "minecraft:lilac",
    "minecraft:tall_grass",
    "minecraft:large_fern",
    "minecraft:rose_bush",
    "minecraft:peony",
];

fn clockwise(facing: &str) -> Option<[i32; 3]> {
    Some(match facing {
        "north" => [1, 0, 0],
        "east" => [0, 0, 1],
        "south" => [-1, 0, 0],
        "west" => [0, 0, -1],
        _ => return None,
    })
}

fn mcedit(root: &Tag) -> Result<Schematic, String> {
    let size = dims(root)?;
    let volume = check_volume(size)? as usize;
    let Some(Tag::ByteArray(blocks)) = root.get("Blocks") else { return Err("no Blocks".into()) };
    let Some(Tag::ByteArray(data)) = root.get("Data") else { return Err("no Data".into()) };
    if blocks.len() < volume || data.len() < volume {
        return Err("Blocks or Data shorter than the size says".into());
    }
    let add = match root.get("AddBlocks") {
        Some(Tag::ByteArray(a)) => a.as_slice(),
        _ => &[],
    };
    let mut entities: HashMap<[i32; 3], Tag> = HashMap::new();
    for t in root.get("TileEntities").and_then(Tag::list).unwrap_or(&[]) {
        if let (Some(x), Some(y), Some(z)) =
            (t.get("x").and_then(Tag::int), t.get("y").and_then(Tag::int), t.get("z").and_then(Tag::int))
        {
            entities.insert([x as i32, y as i32, z as i32], t.clone());
        }
    }
    let mut states: Vec<BlockState> = Vec::with_capacity(volume);
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let i = cell(size, [x, y, z]);
                let mut id = u32::from(blocks[i] as u8);
                if let Some(&a) = add.get(i >> 1) {
                    let a = a as u8;
                    id |= u32::from(if i & 1 == 0 { a & 0x0F } else { a >> 4 }) << 8;
                }
                let mut s = legacy_state(id << 4 | u32::from(data[i] as u8 & 0x0F));
                let be = entities.get(&[x, y, z]);
                if s.name == "%%FILTER_ME%%" {
                    s = skull(&s, be);
                } else if s.name.ends_with("_bed")
                    && let Some(c) = be.and_then(|t| t.get("color")).and_then(Tag::int)
                {
                    s.name = recolor(&s.name, COLORS[c.clamp(0, 15) as usize]);
                } else if s.name.ends_with("_banner")
                    && let Some(base) = be.and_then(|t| t.get("Base")).and_then(Tag::int)
                {
                    // Dye damage: 0 is black, 15 white.
                    s.name = recolor(&s.name, COLORS[15 - base.clamp(0, 15) as usize]);
                }
                states.push(s);
            }
        }
    }
    fix_legacy(size, &mut states);
    let mut palette = Palette::new();
    let indices: Vec<u32> = states.into_iter().map(|s| palette.id(s)).collect();
    let block_entities = entities.into_iter().collect();
    let region = Region::from_indices("Main", size, palette.states, &indices, block_entities);
    Ok(finish(String::new(), String::new(), FLATTENED_DATA_VERSION, region))
}

/// What 1.12 worked out from neighbours when it read a block: the upper half of a door has
/// the lower half's facing (and the lower the upper's hinge), a tall plant's upper half is the
/// lower's plant, grass under snow is snowy, and two chests side by side are one double chest.
fn fix_legacy(size: [i32; 3], states: &mut [BlockState]) {
    let at = |p: [i32; 3]| (0..3).all(|i| p[i] >= 0 && p[i] < size[i]).then(|| cell(size, p));
    let mut changes: Vec<(usize, BlockState)> = Vec::new();
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let i = cell(size, [x, y, z]);
                let s = &states[i];
                let below = at([x, y - 1, z]).map(|j| &states[j]);
                let above = at([x, y + 1, z]).map(|j| &states[j]);
                if s.name.ends_with("_door") && s.prop("half") == Some("upper") {
                    if let Some(lower) = below.filter(|b| b.name == s.name && b.prop("half") == Some("lower")) {
                        let mut up = s.clone();
                        let mut low = lower.clone();
                        for k in ["facing", "open"] {
                            if let Some(v) = lower.prop(k) {
                                up.properties.insert(k.into(), v.into());
                            }
                        }
                        for k in ["hinge", "powered"] {
                            if let Some(v) = s.prop(k) {
                                low.properties.insert(k.into(), v.into());
                            }
                        }
                        changes.push((i, up));
                        changes.push((cell(size, [x, y - 1, z]), low));
                    }
                } else if TALL_PLANTS.contains(&s.name.as_str()) && s.prop("half") == Some("upper") {
                    if let Some(lower) = below.filter(|b| TALL_PLANTS.contains(&b.name.as_str())) {
                        let mut up = s.clone();
                        up.name = lower.name.clone();
                        changes.push((i, up));
                    }
                } else if matches!(s.name.as_str(), "minecraft:grass_block" | "minecraft:podzol" | "minecraft:mycelium") {
                    let snowy = above.is_some_and(|a| matches!(a.name.as_str(), "minecraft:snow" | "minecraft:snow_block"));
                    let mut n = s.clone();
                    n.properties.insert("snowy".into(), snowy.to_string());
                    changes.push((i, n));
                } else if matches!(s.name.as_str(), "minecraft:chest" | "minecraft:trapped_chest")
                    && let Some(facing) = s.prop("facing")
                    && let Some(cw) = clockwise(facing)
                {
                    let same = |d: [i32; 3]| {
                        at([x + d[0], y, z + d[2]])
                            .is_some_and(|j| states[j].name == s.name && states[j].prop("facing") == Some(facing))
                    };
                    let t = if same(cw) {
                        "left"
                    } else if same(cw.map(|v| -v)) {
                        "right"
                    } else {
                        "single"
                    };
                    let mut n = s.clone();
                    n.properties.insert("type".into(), t.into());
                    changes.push((i, n));
                }
            }
        }
    }
    for (i, s) in changes {
        states[i] = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(entries: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    #[test]
    fn sponge_v2_and_v3() {
        // 2x1x2: stone, air / stairs, stone; index 128 needs a two-byte varint.
        let mut palette = vec![
            ("minecraft:stone", Tag::Int(0)),
            ("minecraft:air", Tag::Int(1)),
            ("minecraft:oak_stairs[facing=east,half=bottom]", Tag::Int(128)),
        ];
        let data = vec![0i8, 1, -128, 1, 0];
        let v2 = compound(vec![
            ("Version", Tag::Int(2)),
            ("DataVersion", Tag::Int(3465)),
            ("Width", Tag::Short(2)),
            ("Height", Tag::Short(1)),
            ("Length", Tag::Short(2)),
            ("Palette", compound(palette.clone())),
            ("BlockData", Tag::ByteArray(data.clone())),
        ]);
        let (s, f) = load(&nbt::write_gzip("Schematic", &v2)).unwrap();
        assert_eq!(f, Format::Sponge(2));
        assert_eq!(s.data_version, 3465);
        let r = &s.regions[0];
        assert_eq!(r.state([0, 0, 0]).name, "minecraft:stone");
        assert!(r.state([1, 0, 0]).is_air());
        assert_eq!(r.state([0, 0, 1]).prop("facing"), Some("east"));
        assert_eq!(r.state([1, 0, 1]).name, "minecraft:stone");
        assert_eq!(s.metadata.total_blocks, 3);

        palette.truncate(3);
        let v3 = compound(vec![(
            "Schematic",
            compound(vec![
                ("Version", Tag::Int(3)),
                ("DataVersion", Tag::Int(3700)),
                ("Width", Tag::Short(2)),
                ("Height", Tag::Short(1)),
                ("Length", Tag::Short(2)),
                ("Blocks", compound(vec![("Palette", compound(palette)), ("Data", Tag::ByteArray(data))])),
            ]),
        )]);
        let (s3, f3) = load(&nbt::write_gzip("", &v3)).unwrap();
        assert_eq!(f3, Format::Sponge(3));
        assert_eq!(s3.regions[0].state([0, 0, 1]), r.state([0, 0, 1]));
    }

    #[test]
    fn structure_nbt() {
        let state = |name: &str| compound(vec![("Name", Tag::String(name.into()))]);
        let block = |p: [i32; 3], s: i32| {
            compound(vec![("pos", Tag::List(p.iter().map(|&v| Tag::Int(v)).collect())), ("state", Tag::Int(s))])
        };
        let root = compound(vec![
            ("DataVersion", Tag::Int(3465)),
            ("size", Tag::List(vec![Tag::Int(2), Tag::Int(2), Tag::Int(1)])),
            ("palette", Tag::List(vec![state("minecraft:dirt"), state("minecraft:air")])),
            ("blocks", Tag::List(vec![block([0, 0, 0], 0), block([1, 1, 0], 1)])),
        ]);
        let (s, f) = load(&nbt::write_gzip("", &root)).unwrap();
        assert_eq!(f, Format::Structure);
        let r = &s.regions[0];
        assert_eq!(r.state([0, 0, 0]).name, "minecraft:dirt");
        assert!(r.state([1, 1, 0]).is_air());
        assert!(r.state([1, 0, 0]).is_void(), "unlisted cells are left alone");
        assert_eq!(s.metadata.total_blocks, 1);
    }

    #[test]
    fn mcedit_legacy() {
        // 2 wide, 2 high, 1 long: oak door (lower facing north + upper hinge right),
        // granite and a sunflower's upper half over its lower half.
        let blocks: Vec<i8> = vec![64, -81, 64, -81]; // 175 = double_plant, as i8
        let data: Vec<i8> = vec![3, 0, 9, 8];
        let root = compound(vec![
            ("Width", Tag::Short(2)),
            ("Height", Tag::Short(2)),
            ("Length", Tag::Short(1)),
            ("Materials", Tag::String("Alpha".into())),
            ("Blocks", Tag::ByteArray(blocks)),
            ("Data", Tag::ByteArray(data)),
        ]);
        let (s, f) = load(&nbt::write_gzip("Schematic", &root)).unwrap();
        assert_eq!(f, Format::McEdit);
        assert_eq!(s.data_version, FLATTENED_DATA_VERSION);
        let r = &s.regions[0];
        let lower = r.state([0, 0, 0]);
        let upper = r.state([0, 1, 0]);
        assert_eq!(lower.name, "minecraft:oak_door");
        assert_eq!(upper.prop("half"), Some("upper"));
        assert_eq!(upper.prop("facing"), lower.prop("facing"));
        assert_eq!(lower.prop("hinge"), Some("right"));
        assert_eq!(r.state([1, 0, 0]).name, "minecraft:sunflower");
        assert_eq!(r.state([1, 1, 0]).name, "minecraft:sunflower");
        assert_eq!(r.state([1, 1, 0]).prop("half"), Some("upper"));
        assert_eq!(legacy_state(1 << 4 | 1).name, "minecraft:granite");
        // An id without its own line: the block's first state.
        assert_eq!(legacy_state(1 << 4 | 15).name, "minecraft:stone");
    }

    #[test]
    fn mcedit_chests_and_colors() {
        assert_eq!(recolor("minecraft:red_bed", "light_blue"), "minecraft:light_blue_bed");
        assert_eq!(recolor("minecraft:light_gray_wall_banner", "black"), "minecraft:black_wall_banner");
        // Two chests facing south (data 3) side by side along x.
        let root = compound(vec![
            ("Width", Tag::Short(2)),
            ("Height", Tag::Short(1)),
            ("Length", Tag::Short(1)),
            ("Blocks", Tag::ByteArray(vec![54, 54])),
            ("Data", Tag::ByteArray(vec![3, 3])),
        ]);
        let (s, _) = load(&nbt::write_gzip("Schematic", &root)).unwrap();
        let r = &s.regions[0];
        assert_eq!(r.state([0, 0, 0]).prop("facing"), Some("south"));
        // Facing south, clockwise is west: the chest at x=1 has its partner at x=0 (left).
        assert_eq!(r.state([1, 0, 0]).prop("type"), Some("left"));
        assert_eq!(r.state([0, 0, 0]).prop("type"), Some("right"));
    }
}
