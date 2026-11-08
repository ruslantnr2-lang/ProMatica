//! Promatica: the game-independent half of the mod.
//!
//! The helpers and the `promatica` module compiled into `horizon-client` read the world and draw;
//! everything that doesn't need the game lives here, so it is tested with a plain `cargo test`
//! and compiled once by Cargo.

pub mod bitarray;
pub mod formats;
mod legacy_blocks;
pub mod materials;
pub mod nbt;
pub mod placement;
pub mod printer;
pub mod schematic;
pub mod transform;
pub mod verify;

pub use placement::{LayerMode, Placement};
pub use schematic::{BlockState, Schematic};
pub use transform::{Mirror, Rotation};
pub use verify::{Status, Tally};

fn is_schematic(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    formats::EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

/// `name` without its ending.
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// Lists the schematic files in `dir` (all formats in [`formats::EXTENSIONS`], non-recursive,
/// sorted by name).
pub fn list_schematics(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            is_schematic(&name).then_some(name)
        })
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names
}

/// Finds a schematic by exact file name, name without extension, or unique prefix.
pub fn find_schematic(dir: &std::path::Path, query: &str) -> Result<String, String> {
    let names = list_schematics(dir);
    let q = query.to_ascii_lowercase();
    if let Some(n) = names.iter().find(|n| n.to_ascii_lowercase() == q) {
        return Ok(n.clone());
    }
    // `house` for `house.schem`; with `house.litematic` next to it, the .litematic wins.
    let same: Vec<&String> = names.iter().filter(|n| stem(n).to_ascii_lowercase() == q).collect();
    if let Some(n) = same.iter().find(|n| n.to_ascii_lowercase().ends_with(".litematic")).or(same.first()) {
        return Ok((*n).clone());
    }
    let hits: Vec<&String> = names.iter().filter(|n| n.to_ascii_lowercase().starts_with(&q)).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("no schematic \"{query}\" in {}", dir.display())),
        many => Err(format!(
            "\"{query}\" matches {}: {}",
            many.len(),
            many.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}
