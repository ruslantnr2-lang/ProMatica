//! The `.litematic` format: gzip NBT with `Metadata` and `Regions`, each region a palette and a
//! packed `BlockStates` long array indexed `y * sizeX * sizeZ + z * sizeX + x` from the region's
//! minimum corner.

use crate::bitarray::BitArray;
use crate::nbt::{self, Tag};
use std::collections::BTreeMap;

/// A block state as the file names it: `minecraft:oak_stairs` and its properties.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl BlockState {
    pub fn air() -> Self {
        Self { name: "minecraft:air".into(), properties: BTreeMap::new() }
    }
    pub fn new(name: &str, props: &[(&str, &str)]) -> Self {
        Self {
            name: name.into(),
            properties: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
    /// `structure_void` cells are skipped when placing, as Litematica does.
    pub fn is_void(&self) -> bool {
        self.name == "minecraft:structure_void"
    }
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }
    /// `minecraft:oak_stairs[facing=north,half=bottom]`.
    pub fn to_string_full(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        let props: Vec<String> = self.properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{}[{}]", self.name, props.join(","))
    }
    /// Parses `name[k=v,...]`, adding `minecraft:` when the namespace is missing.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (name, rest) = match text.find('[') {
            Some(i) => (&text[..i], Some(&text[i + 1..])),
            None => (text, None),
        };
        if name.is_empty() {
            return None;
        }
        let name = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        let mut properties = BTreeMap::new();
        if let Some(rest) = rest {
            let rest = rest.strip_suffix(']')?;
            for pair in rest.split(',').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=')?;
                properties.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        Some(Self { name, properties })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub name: String,
    pub author: String,
    pub description: String,
    pub region_count: i64,
    pub total_volume: i64,
    pub total_blocks: i64,
    pub enclosing_size: [i32; 3],
    pub time_created: i64,
    pub time_modified: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub name: String,
    /// `Position` as written: one corner of the box, relative to the schematic's origin.
    pub position: [i32; 3],
    /// `Size` as written (components may be negative).
    pub raw_size: [i32; 3],
    /// The box's minimum corner relative to the schematic origin.
    pub min: [i32; 3],
    /// `abs(Size)`.
    pub size: [i32; 3],
    pub palette: Vec<BlockState>,
    blocks: BitArray,
    /// Block entity NBT by position relative to `min`.
    pub block_entities: Vec<([i32; 3], Tag)>,
    pub entity_count: usize,
}

/// Builds a region cell by cell straight into the packed array (no index per cell on the side):
/// the array starts at 2 bits per entry and widens as the palette grows. Air is index 0, which
/// every cell holds until it is set.
#[derive(Clone, Debug)]
pub struct RegionBuilder {
    size: [i32; 3],
    palette: Vec<BlockState>,
    index: std::collections::HashMap<BlockState, u32>,
    blocks: BitArray,
    non_air: u64,
}

impl RegionBuilder {
    pub fn new(size: [i32; 3]) -> Self {
        let size = size.map(|v| v.max(1));
        let volume = size.iter().map(|&v| v as u64).product();
        let air = BlockState::air();
        Self {
            size,
            palette: vec![air.clone()],
            index: [(air, 0)].into_iter().collect(),
            blocks: BitArray::new(2, volume),
            non_air: 0,
        }
    }
    /// The palette index of `state`, added when new (every kind of air is index 0).
    pub fn intern(&mut self, state: BlockState) -> u32 {
        if state.is_air() {
            return 0;
        }
        if let Some(&i) = self.index.get(&state) {
            return i;
        }
        let i = self.palette.len() as u32;
        self.palette.push(state.clone());
        self.index.insert(state, i);
        self.blocks.widen(BitArray::bits_for(self.palette.len()));
        i
    }
    /// Sets the cell at `local` (x fastest, then z, then y) to palette index `i` from `intern`.
    pub fn set(&mut self, local: [i32; 3], i: u32) {
        let [sx, _, sz] = self.size.map(|v| v as u64);
        let at = local[1] as u64 * sx * sz + local[2] as u64 * sx + local[0] as u64;
        let was = self.blocks.get(at);
        self.non_air = self.non_air + u64::from(i != 0) - u64::from(was != 0);
        self.blocks.set(at, i);
    }
    /// Cells that aren't air.
    pub fn non_air(&self) -> u64 {
        self.non_air
    }
    /// Palette entries without air.
    pub fn states(&self) -> usize {
        self.palette.len() - 1
    }
    pub fn finish(self, name: &str, block_entities: Vec<([i32; 3], Tag)>) -> Region {
        Region {
            name: name.into(),
            position: [0; 3],
            raw_size: self.size,
            min: [0; 3],
            size: self.size,
            palette: self.palette,
            blocks: self.blocks,
            block_entities,
            entity_count: 0,
        }
    }
}

impl Region {
    pub fn volume(&self) -> u64 {
        self.size.iter().map(|&v| v as u64).product()
    }
    fn index(&self, x: i32, y: i32, z: i32) -> u64 {
        let [sx, _, sz] = self.size.map(|v| v as u64);
        y as u64 * sx * sz + z as u64 * sx + x as u64
    }
    /// The palette index at `local` (relative to `min`), 0 (air) outside the box.
    pub fn palette_index(&self, local: [i32; 3]) -> u32 {
        let [x, y, z] = local;
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return 0;
        }
        let i = self.blocks.get(self.index(x, y, z));
        if (i as usize) < self.palette.len() { i } else { 0 }
    }
    /// How many cells use each palette entry (one pass over the packed array).
    pub fn histogram(&self) -> Vec<u64> {
        let mut h = vec![0u64; self.palette.len()];
        for i in 0..self.volume() {
            let v = self.blocks.get(i) as usize;
            let n = h.len();
            h[if v < n { v } else { 0 }] += 1;
        }
        h
    }
    /// Cells that are neither air nor structure void.
    pub fn block_count(&self) -> u64 {
        let h = self.histogram();
        self.palette.iter().zip(h).filter(|(s, _)| !s.is_air() && !s.is_void()).map(|(_, n)| n).sum()
    }
    /// A region at the origin from a palette and its packed cells (Litematica's order).
    pub(crate) fn from_bits(
        name: &str,
        size: [i32; 3],
        palette: Vec<BlockState>,
        blocks: BitArray,
        block_entities: Vec<([i32; 3], Tag)>,
    ) -> Self {
        Self {
            name: name.into(),
            position: [0; 3],
            raw_size: size,
            min: [0; 3],
            size,
            palette,
            blocks,
            block_entities,
            entity_count: 0,
        }
    }
    pub fn state(&self, local: [i32; 3]) -> &BlockState {
        &self.palette[self.palette_index(local) as usize]
    }
    /// A region at the origin from a palette and one palette index per cell, in Litematica's
    /// order (x fastest, then z, then y). Used by the other file formats.
    pub fn from_indices(
        name: &str,
        size: [i32; 3],
        palette: Vec<BlockState>,
        indices: &[u32],
        block_entities: Vec<([i32; 3], Tag)>,
    ) -> Self {
        let palette = if palette.is_empty() { vec![BlockState::air()] } else { palette };
        let mut blocks = BitArray::new(BitArray::bits_for(palette.len()), indices.len() as u64);
        for (i, &v) in indices.iter().enumerate() {
            if v != 0 {
                blocks.set(i as u64, v);
            }
        }
        Self {
            name: name.into(),
            position: [0; 3],
            raw_size: size,
            min: [0; 3],
            size,
            palette,
            blocks,
            block_entities,
            entity_count: 0,
        }
    }

    /// Builds a region in memory (tests).
    pub fn from_fn(
        name: &str,
        position: [i32; 3],
        size: [i32; 3],
        mut f: impl FnMut([i32; 3]) -> BlockState,
    ) -> Self {
        let abs = size.map(i32::abs);
        let end = [0, 1, 2].map(|i| position[i] + if size[i] >= 0 { size[i] - 1 } else { size[i] + 1 });
        let min = [0, 1, 2].map(|i| position[i].min(end[i]));
        let mut palette = vec![BlockState::air()];
        let mut indices = Vec::new();
        for y in 0..abs[1] {
            for z in 0..abs[2] {
                for x in 0..abs[0] {
                    let s = f([x, y, z]);
                    let i = palette.iter().position(|p| *p == s).unwrap_or_else(|| {
                        palette.push(s);
                        palette.len() - 1
                    });
                    indices.push(i as u32);
                }
            }
        }
        let mut blocks = BitArray::new(BitArray::bits_for(palette.len()), indices.len() as u64);
        for (i, v) in indices.into_iter().enumerate() {
            blocks.set(i as u64, v);
        }
        Self {
            name: name.into(),
            position,
            raw_size: size,
            min,
            size: abs,
            palette,
            blocks,
            block_entities: Vec::new(),
            entity_count: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Schematic {
    pub version: i64,
    pub data_version: i64,
    pub metadata: Metadata,
    pub regions: Vec<Region>,
}

/// Upper bound on a file's unpacked size: a region of `MAX_VOLUME` cells at 32 bits each.
pub const MAX_UNPACKED: usize = 8 << 30;
/// Upper bound on a region's volume, the same for saving and loading: 2^31 cells, so an index
/// fits the long array's i32 length at any width (memory is the real limit: a few bits a cell).
pub const MAX_VOLUME: u64 = 1 << 31;

pub(crate) fn vec3(tag: Option<&Tag>) -> Option<[i32; 3]> {
    let t = tag?;
    Some([t.get("x")?.int()? as i32, t.get("y")?.int()? as i32, t.get("z")?.int()? as i32])
}

pub(crate) fn read_state(tag: &Tag) -> Result<BlockState, String> {
    let name = tag.get("Name").and_then(Tag::str).ok_or("palette entry without Name")?;
    let mut properties = BTreeMap::new();
    if let Some(props) = tag.get("Properties").and_then(Tag::compound) {
        for (k, v) in props {
            if let Some(v) = v.str() {
                properties.insert(k.clone(), v.to_string());
            }
        }
    }
    Ok(BlockState { name: name.to_string(), properties })
}

impl Schematic {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let root = nbt::read_file_bytes(bytes, MAX_UNPACKED)?;
        Self::from_nbt(root)
    }

    /// Takes the tree apart: each region's block array is kept, not copied.
    pub fn from_nbt(mut root: Tag) -> Result<Self, String> {
        let version = root.get("Version").and_then(Tag::int).ok_or("not a .litematic file (no Version)")?;
        if !(1..=7).contains(&version) {
            return Err(format!("unsupported .litematic version {version}"));
        }
        let data_version = root.get("MinecraftDataVersion").and_then(Tag::int).unwrap_or(0);
        let m = root.get("Metadata");
        let s = |k: &str| m.and_then(|m| m.get(k)).and_then(Tag::str).unwrap_or("").to_string();
        let i = |k: &str| m.and_then(|m| m.get(k)).and_then(Tag::int).unwrap_or(0);
        let metadata = Metadata {
            name: s("Name"),
            author: s("Author"),
            description: s("Description"),
            region_count: i("RegionCount"),
            total_volume: i("TotalVolume"),
            total_blocks: i("TotalBlocks"),
            enclosing_size: vec3(m.and_then(|m| m.get("EnclosingSize"))).unwrap_or([0; 3]),
            time_created: i("TimeCreated"),
            time_modified: i("TimeModified"),
        };
        let Some(Tag::Compound(regions_tag)) = root.remove("Regions") else { return Err("no Regions".into()) };
        let mut regions = Vec::new();
        for (name, mut r) in regions_tag {
            let position = vec3(r.get("Position")).ok_or_else(|| format!("region {name}: no Position"))?;
            let raw_size = vec3(r.get("Size")).ok_or_else(|| format!("region {name}: no Size"))?;
            let size = raw_size.map(i32::abs);
            let end = [0, 1, 2].map(|i| {
                position[i] + if raw_size[i] >= 0 { raw_size[i] - 1 } else { raw_size[i] + 1 }
            });
            let min = [0, 1, 2].map(|i| position[i].min(end[i]));
            let volume: u64 = size.iter().map(|&v| v as u64).product();
            if volume > MAX_VOLUME {
                return Err(format!("region {name} is too large ({volume} blocks)"));
            }
            let palette: Vec<BlockState> = r
                .get("BlockStatePalette")
                .and_then(Tag::list)
                .ok_or_else(|| format!("region {name}: no BlockStatePalette"))?
                .iter()
                .map(read_state)
                .collect::<Result<_, _>>()?;
            let palette = if palette.is_empty() { vec![BlockState::air()] } else { palette };
            let longs = match r.remove("BlockStates") {
                Some(Tag::LongArray(v)) => v,
                _ => Vec::new(),
            };
            let bits = BitArray::bits_for(palette.len());
            let blocks = BitArray::from_vec(longs, bits, volume)
                .map_err(|e| format!("region {name}: {e}"))?;
            let block_entities = r
                .get("TileEntities")
                .and_then(Tag::list)
                .unwrap_or(&[])
                .iter()
                .filter_map(|t| {
                    let p = [t.get("x")?.int()? as i32, t.get("y")?.int()? as i32, t.get("z")?.int()? as i32];
                    Some((p, t.clone()))
                })
                .collect();
            let entity_count = r.get("Entities").and_then(Tag::list).map_or(0, <[Tag]>::len);
            regions.push(Region {
                name: name.clone(),
                position,
                raw_size,
                min,
                size,
                palette,
                blocks,
                block_entities,
                entity_count,
            });
        }
        Ok(Self { version, data_version, metadata, regions })
    }

    /// Writes the schematic as a `.litematic`: gzip NBT, version 7 (sub-version 1), as Litematica
    /// writes it since 1.20.5.
    pub fn to_bytes(&self) -> Vec<u8> {
        let v3 = |p: [i32; 3]| {
            Tag::Compound(BTreeMap::from([
                ("x".into(), Tag::Int(p[0])),
                ("y".into(), Tag::Int(p[1])),
                ("z".into(), Tag::Int(p[2])),
            ]))
        };
        let mut regions = BTreeMap::new();
        for r in &self.regions {
            let palette = r
                .palette
                .iter()
                .map(|s| {
                    let mut m = BTreeMap::from([("Name".to_string(), Tag::String(s.name.clone()))]);
                    if !s.properties.is_empty() {
                        m.insert(
                            "Properties".into(),
                            Tag::Compound(
                                s.properties.iter().map(|(k, v)| (k.clone(), Tag::String(v.clone()))).collect(),
                            ),
                        );
                    }
                    Tag::Compound(m)
                })
                .collect();
            regions.insert(
                r.name.clone(),
                Tag::Compound(BTreeMap::from([
                    ("Position".into(), v3(r.position)),
                    ("Size".into(), v3(r.raw_size)),
                    ("BlockStatePalette".into(), Tag::List(palette)),
                    (
                        "BlockStates".into(),
                        Tag::LongArray(r.blocks.longs().iter().map(|&v| v as i64).collect()),
                    ),
                    ("TileEntities".into(), Tag::List(vec![])),
                    ("Entities".into(), Tag::List(vec![])),
                    ("PendingBlockTicks".into(), Tag::List(vec![])),
                ])),
            );
        }
        let m = &self.metadata;
        let metadata = Tag::Compound(BTreeMap::from([
            ("Name".into(), Tag::String(m.name.clone())),
            ("Author".into(), Tag::String(m.author.clone())),
            ("Description".into(), Tag::String(m.description.clone())),
            ("RegionCount".into(), Tag::Int(self.regions.len() as i32)),
            ("TotalVolume".into(), Tag::Int(m.total_volume as i32)),
            ("TotalBlocks".into(), Tag::Int(m.total_blocks as i32)),
            ("TimeCreated".into(), Tag::Long(m.time_created)),
            ("TimeModified".into(), Tag::Long(m.time_modified)),
            ("EnclosingSize".into(), v3(m.enclosing_size)),
        ]));
        let root = Tag::Compound(BTreeMap::from([
            ("MinecraftDataVersion".into(), Tag::Int(self.data_version as i32)),
            ("Version".into(), Tag::Int(7)),
            ("SubVersion".into(), Tag::Int(1)),
            ("Metadata".into(), metadata),
            ("Regions".into(), Tag::Compound(regions)),
        ]));
        nbt::write_gzip("", &root)
    }

    /// The bounding box of all regions, relative to the origin: (min, max inclusive).
    pub fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for r in &self.regions {
            for i in 0..3 {
                lo[i] = lo[i].min(r.min[i]);
                hi[i] = hi[i].max(r.min[i] + r.size[i] - 1);
            }
        }
        if self.regions.is_empty() {
            return ([0; 3], [0; 3]);
        }
        (lo, hi)
    }

    /// Non-air, non-void blocks in all regions.
    pub fn block_count(&self) -> u64 {
        self.regions.iter().map(Region::block_count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Schematic {
        // A region whose Size is negative in x and z: it extends from Position towards -x/-z.
        let region = Region::from_fn("main", [2, 0, 3], [-3, 2, -4], |[x, y, z]| {
            if y == 0 {
                BlockState::new("minecraft:stone", &[])
            } else if x == 0 && z == 0 {
                BlockState::new("minecraft:oak_stairs", &[("facing", "north"), ("half", "bottom")])
            } else {
                BlockState::air()
            }
        });
        Schematic {
            version: 6,
            data_version: 4000,
            metadata: Metadata { name: "Test".into(), author: "me".into(), ..Default::default() },
            regions: vec![region],
        }
    }

    #[test]
    fn negative_size_gives_min_corner() {
        let s = sample();
        let r = &s.regions[0];
        assert_eq!(r.min, [0, 0, 0]);
        assert_eq!(r.size, [3, 2, 4]);
        assert_eq!(s.bounds(), ([0, 0, 0], [2, 1, 3]));
    }

    #[test]
    fn file_round_trip() {
        let s = sample();
        let back = Schematic::from_bytes(&s.to_bytes()).unwrap();
        let r = &back.regions[0];
        assert_eq!(back.metadata.name, "Test");
        assert_eq!(r.state([1, 0, 2]).name, "minecraft:stone");
        assert_eq!(r.state([0, 1, 0]).prop("facing"), Some("north"));
        assert!(r.state([1, 1, 1]).is_air());
        assert_eq!(back.block_count(), 3 * 4 + 1);
    }

    #[test]
    fn builder_widens_and_round_trips() {
        // 40 kinds of block: the array widens from 2 to 6 bits on the way.
        let size = [7, 5, 6];
        let mut b = RegionBuilder::new(size);
        let kind = |x: i32, y: i32, z: i32| (x + 7 * z + 3 * y) % 41;
        for y in 0..5 {
            for z in 0..6 {
                for x in 0..7 {
                    let k = kind(x, y, z);
                    let state = if k == 0 { BlockState::new("minecraft:cave_air", &[]) } else { BlockState::new(&format!("test:b{k}"), &[]) };
                    let i = b.intern(state);
                    b.set([x, y, z], i);
                }
            }
        }
        let air = (0..5).flat_map(|y| (0..6).flat_map(move |z| (0..7).map(move |x| (x, y, z)))).filter(|&(x, y, z)| kind(x, y, z) == 0).count() as u64;
        assert_eq!(b.non_air(), 7 * 5 * 6 - air);
        assert_eq!(b.states(), 40);
        let s = Schematic { version: 7, data_version: 4000, metadata: Metadata::default(), regions: vec![b.finish("r", Vec::new())] };
        let back = Schematic::from_bytes(&s.to_bytes()).unwrap();
        let r = &back.regions[0];
        assert_eq!(r.size, size);
        for y in 0..5 {
            for z in 0..6 {
                for x in 0..7 {
                    let k = kind(x, y, z);
                    let name = if k == 0 { "minecraft:air".to_string() } else { format!("test:b{k}") };
                    assert_eq!(r.state([x, y, z]).name, name, "at {x} {y} {z}");
                }
            }
        }

        // Counted in one pass; the light read (metadata, sizes) agrees with the full one.
        let h = r.histogram();
        assert_eq!(h.iter().sum::<u64>(), 7 * 5 * 6);
        assert_eq!(h[0], air);
        assert_eq!(r.block_count(), 7 * 5 * 6 - air);
        let mut s = s;
        s.metadata.total_blocks = (7 * 5 * 6 - air) as i64;
        s.metadata.name = "Builder".into();
        let info = crate::formats::summary(&s.to_bytes()).unwrap();
        assert_eq!((info.name.as_str(), info.size, info.blocks, info.regions), ("Builder", size, 7 * 5 * 6 - air, 1));
    }

    #[test]
    fn parse_state_strings() {
        let s = BlockState::parse("oak_stairs[facing=east,half=top]").unwrap();
        assert_eq!(s.name, "minecraft:oak_stairs");
        assert_eq!(s.prop("half"), Some("top"));
        assert_eq!(s.to_string_full(), "minecraft:oak_stairs[facing=east,half=top]");
        assert!(BlockState::parse("stone[facing").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Schematic::from_bytes(b"not nbt").is_err());
    }
}
