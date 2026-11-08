// Promatica: the game-side module. horizon-mixins inserts this file as
// `pub(crate) mod promatica` into `crates/horizon-client/src/game.rs`, so it is a child of the
// `game` module and may use `Game`'s fields and private methods (the world, the inventory, the
// view, `clip_ray`, `with_level`, `block_model_vertices`...).
//
// The helpers (one per hook) call into here; the game-independent logic is the
// `promatica_core` crate. The overlay is drawn by appending quads to the frame's entity geometry
// (`draw`, called from a HEAD hook on `Game::cart_geometry`), in the entity renderer's
// translucent pass, so it needs no render hook point.

use super::Game;
use horizon_client::extras::Corner;
use horizon_client::extras_hud::Corners;
use horizon_client::hud::HudBatch;
use horizon_client::mesher::Vertex;
use horizon_client_render::gfx::native_render::promatica_bridge as bridge;
use super::promatica_menus as menus;
use horizon_core::geometry::{Direction, Vec3};
use horizon_core::position::BlockPos;
use horizon_world::level::PlaceContext;
use horizon_world::registry::BlockStateId;
use promatica_core::formats::Format;
use promatica_core::materials::{self, Material};
use promatica_core::printer::{self, PrinterSettings, Recent, Stuck};
use promatica_core::verify::{self, Status, Tally};
use promatica_core::{BlockState, LayerMode, Mirror, Placement, Rotation, Schematic};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) const ID: &str = "promatica";
pub(crate) const VERSION: &str = "0.3.0";
pub(crate) const DESCRIPTION: &str = "Schematic overlay, verifier, material list and printer (auto-build) for .litematic, .schem, .schematic and .nbt files. Based on Litematica by masa (maruohon) and Sakura-Ryoko, and the Litematica Printer by aleksilassila. Not made or endorsed by them.";

/// Who Promatica owes its design to (`/promatica credits`, the settings page).
const CREDITS: &[&str] = &[
    "Promatica is a Horizon re-creation of Litematica. All credit for the idea and design goes to:",
    "  Litematica by masa (maruohon), github.com/maruohon/litematica, maintained by Sakura-Ryoko (github.com/sakura-ryoko/litematica)",
    "  Litematica Printer by aleksilassila, github.com/aleksilassila/litematica-printer (maintained by Sakura-Ryoko)",
    "  Horizon by the horizon-rust team, github.com/horizon-rust/horizon",
    "  File formats: Litematica (.litematic), Sponge/WorldEdit (.schem), MCEdit (.schematic), Mojang (structure .nbt)",
    "No code was copied from Litematica or the printer; it is not made or endorsed by their authors.",
];

/// Everything a key can do: (id, label, default key). The id is the menu action's (`menu_action`),
/// the default "" leaves it unbound. Bound on the Keys page; the ids name the settings `key_<id>`.
pub(crate) const BINDS: &[(&str, &str, &str)] = &[
    ("open_menu", "Open the Promatica menu", "key.keyboard.m"),
    ("toggle_render", "Toggle schematic rendering", "key.keyboard.v"),
    ("layer_up", "Layer up", "key.keyboard.page.up"),
    ("layer_down", "Layer down", "key.keyboard.page.down"),
    ("layer_mode", "Cycle layer mode", "key.keyboard.home"),
    ("toggle_printer", "Printer on/off", "key.keyboard.k"),
    ("toggle_easy_place", "Easy place on/off", ""),
    ("toggle_materials", "Material list on/off", "key.keyboard.j"),
    ("move_mode", "Move mode", "key.keyboard.n"),
    ("toggle_tool", "Selection tool on/off", ""),
    ("reload", "Reload the last schematic", ""),
    ("rotate_cw", "Rotate clockwise", ""),
    ("rotate_ccw", "Rotate counter-clockwise", ""),
    ("mirror_next", "Mirror (next axis)", ""),
    ("here", "Move the placement to me", ""),
    ("show_toggle", "Show or hide the placement", ""),
    ("select_next", "Select next placement", ""),
    ("select_prev", "Select previous placement", ""),
    ("unload", "Unload this placement", ""),
    ("unload_all", "Unload all placements", ""),
    ("area_pos1", "Area corner 1 at my position", ""),
    ("area_pos2", "Area corner 2 at my position", ""),
    ("area_clear", "Clear the area", ""),
    ("materials_copy", "Copy the material list", ""),
    ("folder", "Open the schematics folder", ""),
];

/// How long the printer may look for a block it can place, per tick. What it can't place is
/// remembered (`Stuck`), so the next tick goes on from there.
const PRINT_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);
/// Cells the verifier scans per tick.
const SCAN_PER_TICK: u64 = 32_768;
/// Video memory a face of the overlay takes: 4 vertices of 44 bytes (the copy in RAM is let go
/// once uploaded).
const FACE_BYTES: usize = 4 * 44;
/// The sections this close are checked for changes every tick; the rest of a large render
/// radius every `FAR_EVERY` ticks.
const NEAR: i32 = 48;
const FAR_EVERY: u64 = 10;
/// Fully lit (sky 15, block 15 as 16x fixed point): ghosts show their texture's own colours.
const FULL_BRIGHT: u32 = 240 | (240 << 8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Missing blocks as translucent block models, mismatches as coloured boxes.
    Ghost,
    /// Everything as coloured boxes.
    Boxes,
}

#[derive(Clone, Debug)]
struct Settings {
    render: bool,
    /// Ghosts and boxes show through terrain.
    through: bool,
    mode: Mode,
    alpha: f32,
    /// How far around the player the overlay is built, in blocks (no upper bound: the player
    /// decides what the memory is worth), or the game's view distance with `radius_view`.
    radius: i32,
    radius_view: bool,
    /// Most faces the overlay keeps (0: no limit); the nearest are built first.
    max_faces: usize,
    /// Water and lava (whole cubes as ghosts) aren't drawn.
    hide_fluids: bool,
    show_extra: bool,
    show_wrong: bool,
    /// Missing blocks are marked through everything, however far.
    highlight_missing: bool,
    /// Wrong blocks and wrong states too.
    highlight_wrong: bool,
    strict: bool,
    hud: bool,
    materials_hud: bool,
    printer_on: bool,
    printer: PrinterSettings,
    /// Redstone's on/off (powered, lit...) counts as a wrong state.
    power: bool,
    /// The selection tool: `tool_item` in the main hand sets the area's corners.
    tool: bool,
    tool_item: String,
    /// How far the tool reaches, in blocks.
    tool_range: f64,
    /// A click sets the corner on the block in front of the face looked at, not on the block.
    tool_front: bool,
    /// Water and lava stop the tool's ray (else it goes through them to the ground).
    tool_fluids: bool,
    /// The tool's controls show in the HUD while it is in hand.
    tool_hud: bool,
    /// The tool works on placements (select, move, turn) instead of the area's corners.
    tool_placement: bool,
    /// Frames around the placements (the selected one bright, cyan or, locked, orange).
    placement_boxes: bool,
    /// Easy place: holding the right button places the block the schematic wants where the
    /// crosshair points, when it is the item in hand.
    easy_place: bool,
    /// Ticks between two easy placements while the button is held.
    easy_interval: u32,
    /// Easy place may click a ghost block with no real neighbour (the printer has its own).
    easy_air: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            render: true,
            through: false,
            mode: Mode::Ghost,
            alpha: 0.55,
            radius: 32,
            radius_view: false,
            max_faces: 300_000,
            hide_fluids: false,
            show_extra: true,
            show_wrong: true,
            highlight_missing: false,
            highlight_wrong: false,
            strict: false,
            hud: true,
            materials_hud: false,
            printer_on: false,
            printer: PrinterSettings { interval: 3, ..PrinterSettings::default() },
            power: true,
            tool: true,
            tool_item: "minecraft:stick".into(),
            tool_range: 128.0,
            tool_front: false,
            tool_fluids: false,
            tool_hud: true,
            tool_placement: false,
            placement_boxes: true,
            easy_place: false,
            easy_interval: 2,
            easy_air: false,
        }
    }
}

#[derive(Default)]
struct Scan {
    /// The placement scanned: its slot id and revision.
    key: (u64, u64),
    index: u64,
    tally: Tally,
    missing: BTreeMap<String, u64>,
    unloaded: u64,
}

/// The overlay's geometry lives in the render side's `promatica_bridge`, per section; this is
/// what the game side remembers of it.
#[derive(Default)]
struct Mesh {
    /// The world block every section's positions are relative to.
    anchor: [i32; 3],
    /// Built sections: the world section each was built against and its quad count.
    built: HashMap<[i32; 3], (Option<Arc<horizon_client::chunk::Section>>, usize)>,
    /// Quads of all sections.
    quads: usize,
    /// Sections still to build, as of the last tick that went through the whole radius.
    todo: usize,
    /// The far sections that tick found stale, nearest first: the ticks in between build on
    /// from here once the near ones are done.
    queue: std::collections::VecDeque<[i32; 3]>,
    /// The area selection's frame as last handed to the render side: its corners, the anchor,
    /// and what the selection tool has picked (None without the tool in hand).
    area: Option<([Option<[i32; 3]>; 2], [i32; 3], Option<Grab>)>,
    /// The save's frames as last handed over: the chunks framed and the anchor.
    save: Option<(Vec<(i32, i32)>, [i32; 3])>,
    /// The placements' frames as last handed over: box, colour, thickness; and the anchor.
    frames: Option<(Vec<([i32; 3], [i32; 3], u32, u32)>, [i32; 3])>,
    /// What the highlights show now: missing, wrong, only this item.
    marks_key: Option<(bool, bool, Option<String>)>,
    /// Sections with their highlights built: the world section each was built against and how
    /// many blocks it marks.
    marks: HashMap<[i32; 3], (Option<Arc<horizon_client::chunk::Section>>, usize)>,
    /// Blocks marked in all sections.
    marks_count: usize,
    /// The sections still to go through, nearest first.
    marks_queue: std::collections::VecDeque<[i32; 3]>,
    /// `MAX_MARKS` was reached: only the nearest are marked.
    marks_full: bool,
}

/// Most blocks highlighted at once, the nearest ones (about 1 KB of video memory each).
const MAX_MARKS: usize = 50_000;
/// Time the highlights may take per tick to go through sections.
const MARKS_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// The render side's section key for the area selection's frame (no real section is that far out).
const AREA_SECTION: [i32; 3] = [i32::MIN; 3];
/// And for the frames around the placements.
const PLACEMENT_SECTION: [i32; 3] = [i32::MIN, i32::MIN, i32::MIN + 2];

/// One loaded schematic. Several can be loaded at once; one of them is selected, and the
/// commands, move mode, the verifier's HUD and the material list are about that one.
struct Slot {
    /// Unique for the session (never reused): caches key palette entries by it.
    id: u64,
    placement: Placement,
    /// Drawn and printed. A hidden placement stays loaded and remembered.
    shown: bool,
    /// Can't be moved, turned or mirrored (by the menu, commands, move mode or the tool).
    locked: bool,
    /// Every item the placement needs (counted once on load).
    totals: BTreeMap<String, u64>,
}

/// Where a block of a placement comes from: (slot index, (region, palette index)).
type Ref = (usize, (usize, u32));

/// What the game makes of a palette entry (cached per placement revision).
#[derive(Clone, Copy, Debug)]
struct Resolved {
    id: Option<BlockStateId>,
    air: bool,
    /// A full opaque cube (`solid_render`): ghost faces between two are hidden.
    cube: bool,
    /// Water (1) or lava (2) itself, not a waterlogged block; 0 for anything else.
    fluid: u8,
}

#[derive(Default)]
struct State {
    settings_loaded: bool,
    /// The world (and dimension) the placement belongs to: `world_key`.
    world_key: Option<String>,
    /// Where the player is in the menu.
    nav: menus::Nav,
    /// The tick the printer last used each hotbar slot (creative picks reuse the oldest).
    hotbar_used: [u64; 9],
    /// The menu screen: its tab, the file picked on Load, the files listed and what is known of
    /// them, the last message, the origin its fields show.
    ui_tab: usize,
    ui_file: Option<String>,
    ui_files: Vec<(String, u64)>,
    ui_files_at: Option<std::time::Instant>,
    /// What is known of each file (`name|size`): `None` while a thread reads it.
    ui_info: HashMap<String, Option<Result<FileInfo, String>>>,
    ui_info_chan: Option<(
        std::sync::mpsc::Sender<(String, Result<FileInfo, String>)>,
        std::sync::mpsc::Receiver<(String, Result<FileInfo, String>)>,
    )>,
    /// Schematic files being read (`start_load`).
    loads: Vec<LoadJob>,
    ui_message: String,
    ui_origin: Option<[i32; 3]>,
    /// The corners the Save tab's fields show; a save that would overwrite waits for a second click.
    ui_corners: [Option<[i32; 3]>; 2],
    ui_overwrite: Option<String>,
    settings: Settings,
    /// What the chat offers after `load` (files), and after `select` and `unload` (placements).
    completions: (&'static [&'static str], &'static [&'static str]),
    /// The loaded placements, drawn in this order (a later one's blocks win where they overlap).
    slots: Vec<Slot>,
    /// Index into `slots` of the selected placement.
    selected: usize,
    next_id: u64,
    /// Bumped whenever a placement is added, removed, shown, hidden, moved or turned
    /// (`sync_rev`): everything built from them is stale.
    rev: u64,
    /// What `rev` was last bumped for: (id, revision, shown) of every slot.
    rev_of: Vec<(u64, u64, bool)>,
    /// The area selection's two corners (`/hm pos1`, `/hm pos2`), saved with `/hm save`.
    area: [Option<[i32; 3]>; 2],
    layer: LayerMode,
    /// Wanted state → game state id (None: the game doesn't know the block).
    ids: HashMap<BlockState, Option<BlockStateId>>,
    /// Game state id → its name and properties.
    states: HashMap<u32, BlockState>,
    /// Item per wanted state (None: no item, such as air or fire).
    items: HashMap<BlockState, Option<String>>,
    unknown: Vec<String>,
    scan: Scan,
    /// The verifier's last full result, for the slot id `tally_of`.
    tally_of: u64,
    tally: Option<Tally>,
    missing: BTreeMap<String, u64>,
    unloaded: u64,
    /// What the built sections show (placement, layers, display settings): another key builds
    /// them all again.
    mesh_key: Option<(u64, LayerMode, Mode, bool, bool, bool, bool, u32, bool)>,
    /// Ticks of `rebuild_mesh` (the far sections are checked every `FAR_EVERY`).
    mesh_tick: u64,
    mesh: Mesh,
    /// Palette entry (slot id, region, index) -> what the game makes of it, for `resolved_rev`.
    resolved: HashMap<(u64, usize, u32), Resolved>,
    resolved_rev: u64,
    recent: Recent,
    next_print: u64,
    /// On a server: the last click sent its own look and aimed through `forced_hit`;
    /// the next tick gives the server the player's real look back.
    server_look: bool,
    /// Move mode: the wheel and the arrow keys move the placement. Off, nothing but a
    /// command moves it, so it can't be pushed by accident.
    moving: bool,
    /// Wheel steps not yet a whole block (touchpads send fractions).
    wheel: f32,
    /// The save under way (`/hm save`), and the file its thread is writing.
    save: Option<SaveJob>,
    save_write: Option<SaveWrite>,
    /// What the selection tool's wheel moves, and the mouse buttons whose press it took.
    grab: Grab,
    tool_down: [bool; 3],
    /// Each bind's id and key (empty: unbound), read once; `capture` is the bind waiting for a key.
    binds: Option<Vec<(String, String)>>,
    capture: Option<String>,
    /// Easy place: the right button is down; its press was taken (so its release is too); the
    /// tick the next placement may come; the item last asked for and until when that waits (a
    /// server's answer to a swap takes a moment); why it can't place right now.
    right_down: bool,
    easy_taken: bool,
    easy_next: u64,
    easy_pick: Option<(String, u64)>,
    easy_note: String,
    /// The mod's own click is being made: the mouse hook leaves it to the game.
    own_click: bool,
    /// The credits line was shown this session (on the first load).
    credited: bool,
    printer_note: String,
    placed: u64,
    /// Blocks no click could place, until something around them changes.
    stuck: Stuck,
    /// The highlights show only blocks of this item (a session's choice).
    highlight_item: Option<String>,
}

impl State {
    fn sel(&self) -> Option<&Slot> {
        self.slots.get(self.selected)
    }
    /// The selected placement.
    fn placement(&self) -> Option<&Placement> {
        self.sel().map(|s| &s.placement)
    }
    fn placement_mut(&mut self) -> Option<&mut Placement> {
        self.slots.get_mut(self.selected).map(|s| &mut s.placement)
    }
    /// The verifier's last full result, when it is the selected placement's.
    fn tally(&self) -> Option<&Tally> {
        self.tally.as_ref().filter(|_| self.sel().is_some_and(|x| x.id == self.tally_of))
    }
    fn state_of(&self, (slot, r): Ref) -> &BlockState {
        self.slots[slot].placement.palette_state(r)
    }
    /// Bumps `rev` when the placements changed since the last call.
    fn sync_rev(&mut self) {
        let now: Vec<(u64, u64, bool)> = self.slots.iter().map(|s| (s.id, s.placement.revision, s.shown)).collect();
        if now != self.rev_of {
            self.rev_of = now;
            self.rev += 1;
        }
    }
    /// The block wanted at `w`: the last shown placement's with one there. As with regions, a
    /// later placement's air doesn't erase an earlier placement's block.
    fn wanted_at(&self, w: [i32; 3]) -> Option<Ref> {
        let mut found: Option<Ref> = None;
        for (i, slot) in self.slots.iter().enumerate() {
            if !slot.shown {
                continue;
            }
            if let Some(r) = slot.placement.wanted_ref(w)
                && (found.is_none() || !slot.placement.palette_state(r).is_air())
            {
                found = Some((i, r));
            }
        }
        found
    }
    /// The box around every shown placement (min, max inclusive).
    fn shown_bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        self.slots.iter().filter(|s| s.shown).map(|s| s.placement.bounds()).reduce(|(alo, ahi), (blo, bhi)| {
            ([0, 1, 2].map(|i| alo[i].min(blo[i])), [0, 1, 2].map(|i| ahi[i].max(bhi[i])))
        })
    }
    /// The area selection's box, once both corners are set.
    fn area_box(&self) -> Option<([i32; 3], [i32; 3])> {
        let [Some(a), Some(b)] = self.area else { return None };
        Some(([0, 1, 2].map(|i| a[i].min(b[i])), [0, 1, 2].map(|i| a[i].max(b[i]))))
    }
    /// Drops every placement (another world, `/hm unload all`).
    fn drop_all(&mut self) {
        reset_caches(self);
        self.slots.clear();
        self.selected = 0;
        self.settings.printer_on = false;
        self.moving = false;
        self.mesh = Mesh::default();
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(f)
}

// ---------------------------------------------------------------- settings

/// The menu (horizon-menus): the main page and its sub-pages, and the schema of every setting
/// with its default (from `Settings::default()`).
fn menu() -> &'static menus::Menu {
    static MENU: std::sync::OnceLock<menus::Menu> = std::sync::OnceLock::new();
    MENU.get_or_init(|| {
        let d = Settings::default();
        menus::Menu::new("Promatica")
            .page("main", "Promatica", |p| {
                p.list("status", 4);
                p.link("load", "Load schematic");
                p.link("placement", "Placement");
                p.link("selection", "Area selection & saving");
                p.link("printer", "Printer (auto-build)");
                p.link("easy_place", "Easy place");
                p.link("display", "Display");
                p.link("hud", "HUD & material list");
                p.link("credits", "Credits");
            })
            .page("load", "Load schematic", |p| {
                p.action("reload", "Reload the last schematic");
                p.action("folder", "Open the schematics folder");
                p.list("files", 10);
            })
            .page("placement", "Placement", |p| {
                p.list("placement", 9);
                p.action("here", "Move it to my position");
                p.action("rotate_ccw", "Rotate counter-clockwise");
                p.action("unload", "Unload this placement");
                p.action("unload_all", "Unload all placements");
            })
            .page("selection", "Area selection & saving", |p| {
                p.toggle("tool", "Selection tool", d.tool);
                p.choice("tool_mode", "Tool works on", &[("area", "The area"), ("placement", "Placements")], "area");
                p.number("tool_range", "Reach", d.tool_range, (16.0, 256.0, 16.0), 0, " blocks");
                p.choice("tool_target", "Corner on", &[("block", "Block hit"), ("front", "Block in front")], "block");
                p.toggle("tool_fluids", "Stops at water/lava", d.tool_fluids);
                p.toggle("tool_hud", "Controls in the HUD", d.tool_hud);
                p.info("Left click: corner 1. Right click: corner 2.");
                p.info("Middle click: pick a corner. Alt+wheel: move it.");
                p.info("Ctrl+wheel: grow or shrink where you look.");
                p.info("/hm save <name> saves the area.");
            })
            .page("printer", "Printer (auto-build)", |p| {
                p.list("printer", 4);
                p.number("printer_interval", "One block every", f64::from(d.printer.interval), (1.0, 20.0, 1.0), 0, " ticks");
                p.number("printer_reach", "Reach", d.printer.reach, (1.0, 6.0, 0.5), 1, " blocks");
                p.toggle("air_place", "Air place (no neighbour needed)", d.printer.air_place);
            })
            .page("easy_place", "Easy place", |p| {
                p.toggle("easy_place", "Easy place", d.easy_place);
                p.number("easy_interval", "Easy place every", f64::from(d.easy_interval), (1.0, 10.0, 1.0), 0, " ticks");
                p.toggle("easy_air", "Easy air place", d.easy_air);
            })
            .page("display", "Display", |p| {
                p.toggle("render", "Rendering", d.render);
                p.toggle("through", "See through blocks", d.through);
                p.choice("mode", "Mode", &[("ghost", "Ghost blocks"), ("boxes", "Boxes")], "ghost");
                p.percent("alpha", "Ghost opacity", f64::from(d.alpha), 0.1, 1.0, 0.05);
                p.number("radius", "Render radius", f64::from(d.radius), (16.0, 1024.0, 16.0), 0, " blocks");
                p.toggle("radius_view", "Radius = view distance", d.radius_view);
                p.choice("max_faces", "Face limit", FACE_LIMITS, "300k");
                p.toggle("hide_fluids", "Hide water and lava", d.hide_fluids);
                p.toggle("placement_boxes", "Boxes around placements", d.placement_boxes);
                p.toggle("show_extra", "Show extra blocks", d.show_extra);
                p.toggle("show_wrong", "Show wrong blocks", d.show_wrong);
                p.toggle("highlight_missing", "Highlight missing blocks (through everything)", d.highlight_missing);
                p.toggle("highlight_wrong", "Highlight wrong blocks (through everything)", d.highlight_wrong);
                p.toggle("strict", "Strict states (connections, stair shapes)", d.strict);
                p.toggle("power", "Redstone on/off counts as wrong", d.power);
            })
            .page("hud", "HUD & material list", |p| {
                p.toggle("hud", "Info HUD", d.hud);
                p.toggle("materials_hud", "Material list HUD", d.materials_hud);
                p.action("materials", "Copy the material list");
            })
            .page("credits", "Credits", |p| {
                p.info("Promatica re-creates Litematica for Horizon.");
                p.info("Litematica: masa (maruohon)");
                p.info("Maintained by Sakura-Ryoko");
                p.info("Litematica Printer: aleksilassila");
                p.info("Horizon: the horizon-rust team");
                p.info("Not made or endorsed by them.");
                p.action("credits_chat", "Show the full credits in chat");
            })
    })
}

fn load_settings(game: &Game<'_>, s: &mut State) {
    ensure_binds(game, s);
    if s.settings_loaded {
        return;
    }
    s.settings_loaded = true;
    let m = menu();
    let st = &menus::GameStore { game, id: ID };
    let d = Settings::default();
    s.settings = Settings {
        render: m.bool(st, "render"),
        through: m.bool(st, "through"),
        mode: if m.choice(st, "mode") == "boxes" { Mode::Boxes } else { Mode::Ghost },
        alpha: m.number(st, "alpha") as f32,
        // Read as written: `/hm radius` and `/hm faces` go past the menu's choices.
        radius: game.native_setting(ID, "radius").and_then(|v| v.trim().parse::<f64>().ok()).map_or(d.radius, |v| (v as i32).max(16)),
        radius_view: m.bool(st, "radius_view"),
        max_faces: game.native_setting(ID, "max_faces").and_then(|v| parse_faces(&v)).unwrap_or(d.max_faces),
        hide_fluids: m.bool(st, "hide_fluids"),
        show_extra: m.bool(st, "show_extra"),
        show_wrong: m.bool(st, "show_wrong"),
        highlight_missing: m.bool(st, "highlight_missing"),
        highlight_wrong: m.bool(st, "highlight_wrong"),
        strict: m.bool(st, "strict"),
        hud: m.bool(st, "hud"),
        materials_hud: m.bool(st, "materials_hud"),
        // A session's switch: settings are read again after every menu click.
        printer_on: s.settings.printer_on,
        printer: PrinterSettings {
            interval: m.number(st, "printer_interval") as u32,
            reach: m.number(st, "printer_reach"),
            air_place: m.bool(st, "air_place"),
            ..d.printer
        },
        power: m.bool(st, "power"),
        tool: m.bool(st, "tool"),
        tool_item: game
            .native_setting(ID, "tool_item")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or(d.tool_item),
        tool_range: m.number(st, "tool_range"),
        tool_front: m.choice(st, "tool_target") == "front",
        tool_fluids: m.bool(st, "tool_fluids"),
        tool_hud: m.bool(st, "tool_hud"),
        tool_placement: m.choice(st, "tool_mode") == "placement",
        placement_boxes: m.bool(st, "placement_boxes"),
        easy_place: m.bool(st, "easy_place"),
        easy_interval: (m.number(st, "easy_interval") as u32).max(1),
        easy_air: m.bool(st, "easy_air"),
    };
}

/// The face limit's choices in the menu (`/hm faces` takes any number).
const FACE_LIMITS: &[(&str, &str)] =
    &[("300k", "300k (~50 MB)"), ("1m", "1 M (~180 MB)"), ("2m", "2 M (~350 MB)"), ("5m", "5 M (~880 MB)"), ("10m", "10 M (~1.8 GB)"), ("none", "No limit")];

/// A face limit as written: `none` (0), a number, or one with k or m (`300k`, `2.5m`).
fn parse_faces(v: &str) -> Option<usize> {
    let v = v.trim().to_ascii_lowercase();
    if matches!(v.as_str(), "none" | "off" | "unlimited" | "0") {
        return Some(0);
    }
    let (num, times) = match v.strip_suffix('m').or_else(|| v.strip_suffix("mio")) {
        Some(n) => (n, 1_000_000.0),
        None => match v.strip_suffix('k') {
            Some(n) => (n, 1_000.0),
            None => (v.as_str(), 1.0),
        },
    };
    let n = num.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0)? * times;
    Some((n as usize).max(1_000))
}

/// How a face limit is stored: the menu's key when it is one of its choices.
fn faces_key(n: usize) -> String {
    match n {
        0 => "none".into(),
        n if n % 1_000_000 == 0 => format!("{}m", n / 1_000_000),
        n if n % 1_000 == 0 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

fn faces_text(n: usize) -> String {
    match n {
        0 => "no limit".into(),
        n if n >= 1_000_000 => format!("{} M faces", (n as f64 / 1e6 * 10.0).round() / 10.0),
        n => format!("{}k faces", n / 1_000),
    }
}

fn mib(bytes: usize) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.0} MB") }
}

/// The radius the overlay is built in: the setting, or the game's view distance.
fn render_radius(game: &Game<'_>, st: &Settings) -> i32 {
    if st.radius_view { (game.settings.render_distance as i32).max(1) * 16 } else { st.radius }
}

/// Writes `s` in the menu's format (commands change settings too).
fn save_settings(game: &Game<'_>, s: &Settings) {
    let set = |k: &str, v: String| game.native_set_setting(ID, k, &v);
    set("render", s.render.to_string());
    set("through", s.through.to_string());
    set("mode", if s.mode == Mode::Boxes { "boxes" } else { "ghost" }.into());
    set("alpha", s.alpha.to_string());
    set("radius", s.radius.to_string());
    set("radius_view", s.radius_view.to_string());
    set("max_faces", faces_key(s.max_faces));
    set("hide_fluids", s.hide_fluids.to_string());
    set("show_extra", s.show_extra.to_string());
    set("show_wrong", s.show_wrong.to_string());
    set("highlight_missing", s.highlight_missing.to_string());
    set("highlight_wrong", s.highlight_wrong.to_string());
    set("strict", s.strict.to_string());
    set("hud", s.hud.to_string());
    set("materials_hud", s.materials_hud.to_string());
    set("printer_interval", s.printer.interval.to_string());
    set("printer_reach", s.printer.reach.to_string());
    set("air_place", s.printer.air_place.to_string());
    set("power", s.power.to_string());
    set("tool", s.tool.to_string());
    set("tool_item", s.tool_item.clone());
    set("tool_range", s.tool_range.to_string());
    set("tool_target", if s.tool_front { "front" } else { "block" }.into());
    set("tool_fluids", s.tool_fluids.to_string());
    set("tool_hud", s.tool_hud.to_string());
    set("tool_mode", if s.tool_placement { "placement" } else { "area" }.into());
    set("placement_boxes", s.placement_boxes.to_string());
    set("easy_place", s.easy_place.to_string());
    set("easy_interval", s.easy_interval.to_string());
    set("easy_air", s.easy_air.to_string());
}

/// Saves where the placements are: the selected one as the last one (`/promatica reload`),
/// all of them as this world's, which come back when the world is joined again.
///
/// A world's value is `v2`, the selected index, then one line per placement:
/// `x|y|z|rotation|mirror|flags|file` (the file last, so it may hold any character but a line
/// break; flags: 1 shown, 2 locked, so 0 and 1 read as before). Before 0.2.0 it was the one placement as `file|x|y|z|rotation|mirror`, as `last` still is.
fn save_placements(game: &Game<'_>) {
    let (last, world) = with(|s| {
        let last = s.placement().map(|p| {
            format!("{}|{}|{}|{}|{}|{}", p.file_name, p.origin[0], p.origin[1], p.origin[2], p.rotation.name(), p.mirror.name())
        });
        let mut world = if s.slots.is_empty() { String::new() } else { format!("v2\n{}", s.selected) };
        for slot in &s.slots {
            let p = &slot.placement;
            world += &format!(
                "\n{}|{}|{}|{}|{}|{}|{}",
                p.origin[0],
                p.origin[1],
                p.origin[2],
                p.rotation.name(),
                p.mirror.name(),
                u8::from(slot.shown) | u8::from(slot.locked) << 1,
                p.file_name
            );
        }
        (last, world)
    });
    if let Some(last) = last {
        game.native_set_setting(ID, "last", &last);
    }
    if let Some(key) = world_key(game) {
        game.native_set_setting(ID, &key, &world);
    }
}

/// Where a saved placement goes: origin, rotation, mirror.
type At = ([i32; 3], Rotation, Mirror);

/// A world's saved placements (either format): the selected index, and per placement its file,
/// where it goes, and whether it is shown and locked.
fn parse_world(v: &str) -> (usize, Vec<(String, At, (bool, bool))>) {
    let mut lines = v.lines();
    if lines.next() != Some("v2") {
        return (0, parse_saved(v).map(|(file, at)| (file, at, (true, false))).into_iter().collect());
    }
    let selected = lines.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    let entries = lines
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(7, '|').collect();
            if parts.len() != 7 || parts[6].is_empty() {
                return None;
            }
            let n = |i: usize| parts[i].parse::<i32>().ok();
            let at = ([n(0)?, n(1)?, n(2)?], Rotation::parse(parts[3]).unwrap_or_default(), Mirror::parse(parts[4]).unwrap_or_default());
            let flags = parts[5].parse::<u8>().unwrap_or(1);
            Some((parts[6].to_string(), at, (flags & 1 != 0, flags & 2 != 0)))
        })
        .collect();
    (selected, entries)
}

/// The settings key of the world played and its dimension: a single-player world by its save
/// folder, a server by its address. None outside a world.
fn world_key(game: &Game<'_>) -> Option<String> {
    if game.title.is_some() || game.player.is_none() {
        return None;
    }
    let world = match game.server_address() {
        Some(address) if game.integrated.is_none() => format!("server_{address}"),
        _ => format!("world_{}", game.save.root.file_name()?.to_string_lossy()),
    };
    let key = format!("placed.{world}.{:?}", game.dim);
    Some(key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect())
}

/// A saved placement: file, origin, rotation, mirror.
fn parse_saved(v: &str) -> Option<(String, ([i32; 3], Rotation, Mirror))> {
    let parts: Vec<&str> = v.split('|').collect();
    if parts.len() != 6 || parts[0].is_empty() {
        return None;
    }
    let n = |i: usize| parts[i].parse::<i32>().ok();
    let at = ([n(1)?, n(2)?, n(3)?], Rotation::parse(parts[4]).unwrap_or_default(), Mirror::parse(parts[5]).unwrap_or_default());
    Some((parts[0].to_string(), at))
}

/// Joining a world (or another dimension of it): its placements come back where they were left;
/// those of another world go, and so does the area selection.
fn restore_step(game: &mut Game<'_>) {
    let Some(key) = world_key(game) else { return };
    if with(|s| s.world_key.as_deref() == Some(key.as_str())) {
        return;
    }
    with(|s| {
        s.world_key = Some(key.clone());
        s.drop_all();
        s.area = [None; 2];
    });
    bridge::clear();
    let Some(saved) = game.native_setting(ID, &key) else { return };
    let (selected, entries) = parse_world(&saved);
    if entries.is_empty() {
        return;
    }
    let flags = entries.iter().map(|e| e.2).collect();
    let files = entries.into_iter().map(|(file, at, _)| (file, at)).collect();
    let lines = start_load(game, files, Then::Restore { flags, selected }).unwrap_or_else(|e| vec![format!("Couldn't restore: {e}")]);
    for l in lines {
        say(game, &l);
    }
}

// ---------------------------------------------------------------- the mod list

pub(super) fn listed() -> super::native_hooks::NativeMod {
    super::native_hooks::NativeMod {
        id: ID.into(),
        title: format!("Promatica {VERSION}"),
        description: DESCRIPTION.into(),
        // Bound on the mod's own Keys page, not in Options > Mods.
        keys: Vec::new(),
    }
}

/// The mod's page in Options > Mods: the menu's page the player is on.
pub(super) fn settings_buttons(game: &Game<'_>, buttons: &mut Vec<String>) {
    let mut nav = with(|s| {
        load_settings(game, s);
        std::mem::take(&mut s.nav)
    });
    let store = menus::GameStore { game, id: ID };
    buttons.extend(menu().render(&mut nav, &store, &mut |list| menu_list(list)));
    with(|s| s.nav = nav);
}

pub(super) fn settings_click(game: &mut Game<'_>, button: usize) {
    let mut nav = with(|s| std::mem::take(&mut s.nav));
    let reverse = menus::reverse(game);
    let event = {
        let mut store = menus::GameStore { game: &*game, id: ID };
        menu().click(&mut nav, &mut store, button, reverse)
    };
    with(|s| {
        s.nav = nav;
        s.settings_loaded = false;
        load_settings(game, s);
        s.mesh_key = None;
    });
    if let menus::Event::Action(action) = event {
        menu_action(game, &action);
    }
}

/// The rows the menu's lists show now: `(label, action)`, no action for a plain line.
fn menu_list(list: &str) -> Vec<(String, String)> {
    let row = |label: String, action: &str| (label, action.to_string());
    let printer = |on: bool| row(format!("Printer: {} (click to switch)", if on { "ON" } else { "OFF" }), "printer_toggle");
    match list {
        "status" => with(|s| {
            let Some(p) = s.placement() else {
                return vec![row("No schematic loaded: Load schematic below".into(), "")];
            };
            let mut rows = vec![row(format!("Selected: {} ({} of {})", display_name(p), s.selected + 1, s.slots.len()), "")];
            if let Some(t) = s.tally() {
                rows.push(row(format!("{:.1}% done, {} missing", t.percent(), t.missing), ""));
            }
            rows.push(printer(s.settings.printer_on));
            rows
        }),
        "files" => {
            let Ok(dir) = schematics_dir() else { return vec![row("No schematics folder".into(), "")] };
            let names = promatica_core::list_schematics(&dir);
            if names.is_empty() {
                return vec![row("No files yet: .litematic, .schem, .schematic, .nbt".into(), "")];
            }
            names.into_iter().map(|n| (n.clone(), format!("load:{n}"))).collect()
        }
        "placement" => with(|s| {
            let Some(p) = s.placement() else { return vec![row("No schematic loaded".into(), "")] };
            let shown = s.sel().is_some_and(|x| x.shown);
            vec![
                row(format!("{} of {}: {} (click: next)", s.selected + 1, s.slots.len(), display_name(p)), "select_next"),
                row(format!("Shown: {} (click to switch)", if shown { "yes" } else { "no" }), "show_toggle"),
                row(format!("Origin {} {} {}", p.origin[0], p.origin[1], p.origin[2]), ""),
                row(format!("Rotation: {} (click: clockwise)", p.rotation.name()), "rotate_cw"),
                row(format!("Mirror: {} (click: next)", p.mirror.name()), "mirror_next"),
                row(format!("Layers: {} (click: next mode)", s.layer.describe()), "layer_next"),
                row(
                    format!("Move mode: {} (wheel, arrows, PgUp/PgDn)", if s.moving { "ON" } else { "OFF" }),
                    "move_mode",
                ),
            ]
        }),
        "printer" => with(|s| {
            let mut rows = vec![printer(s.settings.printer_on), row(format!("Placed this session: {}", s.placed), "")];
            if !s.printer_note.is_empty() {
                rows.push(row(s.printer_note.clone(), ""));
            }
            rows
        }),
        _ => Vec::new(),
    }
}

/// The next layer mode (all, this layer, above, below), as the key cycles them.
fn next_layer_mode() -> &'static str {
    with(|s| match s.layer {
        LayerMode::All => "here",
        LayerMode::Single(_) => "above",
        LayerMode::Above(_) => "below",
        _ => "all",
    })
}

/// A menu row's action.
fn menu_action(game: &mut Game<'_>, action: &str) {
    let r = match action {
        "folder" => {
            match schematics_dir() {
                Ok(dir) => {
                    let _ = std::fs::create_dir_all(&dir);
                    super::packs::open_folder(&dir);
                }
                Err(e) => say(game, &format!("Error: {e}")),
            }
            return;
        }
        "move_mode" => {
            toggle_move_mode(game);
            return;
        }
        "credits_chat" => {
            for line in CREDITS {
                game.native_chat(line);
            }
            return;
        }
        "mirror_next" => {
            let next = with(|s| match s.placement().map(|p| p.mirror) {
                // x flips x (FRONT_BACK), z flips z (LEFT_RIGHT).
                Some(Mirror::None) => "x",
                Some(Mirror::FrontBack) => "z",
                _ => "none",
            });
            run(game, &["mirror", next])
        }
        "layer_next" | "layer_mode" => {
            let next = next_layer_mode();
            run(game, &["layer", next])
        }
        "reload" => run(game, &["reload"]),
        "here" => run(game, &["origin", "here"]),
        "rotate_cw" => run(game, &["rotate", "cw"]),
        "rotate_ccw" => run(game, &["rotate", "ccw"]),
        "unload" => run(game, &["unload"]),
        "unload_all" => run(game, &["unload", "all"]),
        "select_next" => run(game, &["select", "next"]),
        "show_toggle" => run(game, &["show", "toggle"]),
        "printer_toggle" | "toggle_printer" => run(game, &["printer"]),
        "toggle_render" => run(game, &["render"]),
        "layer_up" => run(game, &["layer", "up"]),
        "layer_down" => run(game, &["layer", "down"]),
        "toggle_easy_place" => run(game, &["easyplace"]),
        "toggle_tool" => {
            let on = with(|s| s.settings.tool);
            run(game, &["tool", if on { "off" } else { "on" }])
        }
        "select_prev" => run(game, &["select", "prev"]),
        "area_pos1" => run(game, &["pos1", "here"]),
        "area_pos2" => run(game, &["pos2", "here"]),
        "area_clear" => run(game, &["area", "clear"]),
        "materials" | "materials_copy" => run(game, &["materials", "copy"]),
        other => match other.strip_prefix("load:") {
            Some(file) => run(game, &["load", file]),
            None => return,
        },
    };
    report(game, r);
}

// ---------------------------------------------------------------- block lookups

fn to_world_state(s: &BlockState) -> horizon_world::chunk::BlockState {
    horizon_world::chunk::BlockState { name: s.name.clone(), properties: s.properties.clone().into() }
}

/// The game's id for a schematic state; properties the game doesn't know are left out.
fn resolve(game: &Game<'_>, s: &BlockState) -> Option<BlockStateId> {
    let registry = &game.pack.assets.registry;
    if let Ok(id) = registry.resolve(&to_world_state(s)) {
        return Some(id);
    }
    let default = registry.default_state(&s.name)?;
    let mut best = default;
    for (k, v) in &s.properties {
        let mut probe = registry.state(best)?.clone();
        if probe.properties.contains_key(k) {
            probe.properties.insert(k.clone(), v.clone());
            if let Some(id) = registry.id(&probe) {
                best = id;
            }
        }
    }
    Some(best)
}

fn wanted_id(game: &Game<'_>, s: &mut State, w: &BlockState) -> Option<BlockStateId> {
    if let Some(id) = s.ids.get(w) {
        return *id;
    }
    let id = resolve(game, w);
    if id.is_none() && !s.unknown.contains(&w.name) {
        s.unknown.push(w.name.clone());
    }
    s.ids.insert(w.clone(), id);
    id
}

fn game_state(game: &Game<'_>, states: &mut HashMap<u32, BlockState>, id: BlockStateId) -> BlockState {
    states
        .entry(id.0)
        .or_insert_with(|| match game.pack.assets.registry.state(id) {
            Some(st) => BlockState { name: st.name.clone(), properties: st.properties.clone().into() },
            None => BlockState::air(),
        })
        .clone()
}

fn item_for(game: &Game<'_>, s: &mut State, w: &BlockState) -> Option<String> {
    if let Some(i) = s.items.get(w) {
        return i.clone();
    }
    let item = match w.name.as_str() {
        "minecraft:water" | "minecraft:bubble_column" => Some("minecraft:water_bucket".to_string()),
        "minecraft:lava" => Some("minecraft:lava_bucket".to_string()),
        name if materials::planted_by(name).is_some() => materials::planted_by(name).map(String::from),
        _ => wanted_id(game, s, w).and_then(|id| {
            let item = &game.res.facts().get(id).item;
            (!item.is_empty() && &**item != "minecraft:air").then(|| item.to_string())
        }),
    };
    s.items.insert(w.clone(), item.clone());
    item
}

fn loaded(game: &Game<'_>, w: [i32; 3]) -> bool {
    let shape = &game.world.shape;
    w[1] >= shape.min_y
        && w[1] < shape.min_y + shape.height
        && game.world.chunks.get(&(w[0] >> 4, w[2] >> 4)).is_some()
}

fn resolve_ref(game: &Game<'_>, s: &mut State, at: Ref) -> Option<Resolved> {
    if s.resolved_rev != s.rev {
        s.resolved.clear();
        s.resolved_rev = s.rev;
    }
    let key = (s.slots.get(at.0)?.id, at.1.0, at.1.1);
    if let Some(v) = s.resolved.get(&key) {
        return Some(*v);
    }
    let state = s.state_of(at).clone();
    let id = wanted_id(game, s, &state);
    let facts = game.res.facts();
    let res = Resolved {
        id,
        air: state.is_air(),
        cube: id.is_some_and(|i| facts.get(i).solid_render),
        fluid: id.map_or(0, |i| {
            use horizon_client::blocks::Fluid;
            let f = facts.get(i);
            match f.fluid {
                Fluid::Water if !f.waterlogged => 1,
                Fluid::Lava => 2,
                _ => 0,
            }
        }),
    };
    s.resolved.insert(key, res);
    Some(res)
}

/// The verifier's verdict at `w` against the shown placements (None: outside them or nothing to
/// say), with where its wanted state is and what the game makes of it.
fn quick_status(game: &Game<'_>, s: &mut State, w: [i32; 3]) -> Option<(Status, Ref, Resolved)> {
    let at = s.wanted_at(w)?;
    judge(game, s, w, at)
}

/// The verdict at `w` against the placement in slot `slot` alone (shown or not).
fn quick_status_in(game: &Game<'_>, s: &mut State, slot: usize, w: [i32; 3]) -> Option<(Status, Ref, Resolved)> {
    let r = s.slots.get(slot)?.placement.wanted_ref(w)?;
    judge(game, s, w, (slot, r))
}

/// The verdict at `w` for the wanted block `at`. The common cases (the right block, air where a
/// block is missing) need no state compared.
fn judge(game: &Game<'_>, s: &mut State, w: [i32; 3], at: Ref) -> Option<(Status, Ref, Resolved)> {
    let res = resolve_ref(game, s, at)?;
    let actual_id = game.world.block(w[0], w[1], w[2]);
    if !res.air && res.id == Some(actual_id) {
        return Some((Status::Correct, at, res));
    }
    let facts = game.res.facts();
    if facts.is_air(actual_id) {
        return (!res.air).then_some((Status::Missing, at, res));
    }
    let f = facts.get(actual_id);
    let replaceable = f.replaceable || (f.fluid != horizon_client::blocks::Fluid::None && !f.waterlogged);
    let wanted = s.state_of(at).clone();
    let actual = game_state(game, &mut s.states, actual_id);
    let status = verify::classify_power(Some(&wanted), &actual, replaceable, s.settings.strict, s.settings.power)?;
    Some((status, at, res))
}

/// The verifier's verdict at `w` against the shown placements, and the state wanted there.
fn status_at(game: &Game<'_>, s: &mut State, w: [i32; 3]) -> Option<(Status, BlockState)> {
    let (status, at, _) = quick_status(game, s, w)?;
    Some((status, s.state_of(at).clone()))
}

// ---------------------------------------------------------------- loading

fn schematics_dir() -> Result<PathBuf, String> {
    let config = Game::native_config_dir()?;
    let dir = config.parent().map_or_else(|| config.join("schematics"), |g| g.join("schematics"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn feet(game: &Game<'_>) -> Option<[i32; 3]> {
    let p = game.native_player()?;
    Some([p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32])
}

fn reset_caches(s: &mut State) {
    s.ids.clear();
    // Keyed by palette index and checked only against the placement's revision, which a new
    // placement starts again at 0: without this a new schematic shows the old one's blocks.
    s.resolved.clear();
    s.items.clear();
    s.unknown.clear();
    s.scan = Scan::default();
    s.tally = None;
    s.missing.clear();
    s.unloaded = 0;
    s.mesh_key = None;
    s.recent.clear();
    s.stuck.clear();
}

/// Counts every item a placement needs (once per load).
fn count_totals(game: &Game<'_>, s: &mut State, p: &Placement, histograms: &[Vec<u64>]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for (ri, histogram) in histograms.iter().enumerate() {
        let palette = &p.palettes()[ri];
        for (i, &n) in histogram.iter().enumerate() {
            let st = &palette[i];
            let per = u64::from(materials::items_for_state(st));
            if n == 0 || per == 0 {
                continue;
            }
            if let Some(item) = item_for(game, s, st) {
                *counts.entry(item).or_insert(0) += n * per;
            }
        }
    }
    counts
}

/// What a placement is called: its schematic's name, else its file's.
fn display_name(p: &Placement) -> String {
    if p.schematic.metadata.name.is_empty() { p.file_name.clone() } else { p.schematic.metadata.name.clone() }
}

/// Brings an older file's states up to this game version with Horizon's data fixer (1.12 names
/// to flattened ones, `grass` to `short_grass`, ...). Returns how many palette entries changed.
fn upgrade_states(schematic: &mut Schematic) -> usize {
    use horizon_world::datafix::{self, schema::Ty};
    use horizon_world::nbt::Tag as N;
    let from = schematic.data_version as i32;
    let to = horizon_world::DATA_VERSION;
    if from <= 0 || from >= to {
        return 0;
    }
    let mut changed = 0;
    for region in &mut schematic.regions {
        for state in &mut region.palette {
            if state.is_air() {
                continue;
            }
            let name = N::String(state.name.as_str().into());
            let mut tag = if state.properties.is_empty() {
                N::compound([("Name", name)])
            } else {
                let props = state.properties.iter().map(|(k, v)| (k.as_str(), N::String(v.as_str().into())));
                N::compound([("Name", name), ("Properties", N::compound(props))])
            };
            datafix::update(Ty::BlockState, &mut tag, from, to);
            // The fixer's newest block state format names the fields `id` and `properties`.
            let Some(N::String(name)) = tag.get("id").or_else(|| tag.get("Name")) else { continue };
            let mut properties = BTreeMap::new();
            if let Some(N::Compound(props)) = tag.get("properties").or_else(|| tag.get("Properties")) {
                for (k, v) in props {
                    if let N::String(v) = v {
                        properties.insert(k.to_string_lossy(), v.to_string_lossy());
                    }
                }
            }
            let new = BlockState { name: name.to_string_lossy(), properties };
            if new != *state {
                *state = new;
                changed += 1;
            }
        }
    }
    schematic.data_version = i64::from(to);
    changed
}

// ---------------------------------------------------------------- reading schematic files

/// A schematic file as read off the game thread: parsed, its block states brought up to date,
/// and its cells counted per palette entry (for the material totals). Placing it is quick.
struct ReadFile {
    file: String,
    schematic: Schematic,
    format: Format,
    from_version: i64,
    upgraded: usize,
    histograms: Vec<Vec<u64>>,
}

fn read_file(dir: &std::path::Path, query: &str) -> Result<ReadFile, String> {
    let file = promatica_core::find_schematic(dir, query)?;
    let bytes = std::fs::read(dir.join(&file)).map_err(|e| format!("{file}: {e}"))?;
    let (mut schematic, format) = promatica_core::formats::load(&bytes).map_err(|e| format!("{file}: {e}"))?;
    drop(bytes);
    let from_version = schematic.data_version;
    let upgraded = upgrade_states(&mut schematic);
    let histograms = schematic.regions.iter().map(|r| r.histogram()).collect();
    Ok(ReadFile { file, schematic, format, from_version, upgraded, histograms })
}

/// What becomes of files once they are read.
enum Then {
    /// `/hm load`: a new placement, selected.
    Load,
    /// `/hm reload`: replaces the placement of that file at that place, else a new one.
    Reload,
    /// Joining a world: its placements back, each shown or hidden, and the one selected.
    Restore { flags: Vec<(bool, bool)>, selected: usize },
}

/// Files a thread of their own is reading (`start_load`); `load_step` places them.
struct LoadJob {
    files: Vec<(String, At)>,
    then: Then,
    /// The world they are for: left before they are in, they are dropped.
    world: Option<String>,
    started: std::time::Instant,
    bytes: u64,
    done: std::sync::mpsc::Receiver<Vec<Result<ReadFile, String>>>,
}

impl LoadJob {
    fn label(&self) -> String {
        match self.files.as_slice() {
            [(f, _)] => f.clone(),
            many => format!("{} placements", many.len()),
        }
    }
}

/// How long a command waits for its files before they go on loading in the background: small
/// files are placed at once, as before.
const LOAD_WAIT: std::time::Duration = std::time::Duration::from_millis(150);

/// Reads `files` (names or searches in the schematics folder) in a thread. Read within
/// `LOAD_WAIT`, they are placed right away and the result told; else the line says they are on
/// their way, and `load_step` places them once they are in.
fn start_load(game: &mut Game<'_>, files: Vec<(String, At)>, then: Then) -> Result<Vec<String>, String> {
    use std::sync::mpsc::RecvTimeoutError;
    let dir = schematics_dir()?;
    let queries: Vec<String> = files.iter().map(|(f, _)| f.clone()).collect();
    let bytes = queries
        .iter()
        .filter_map(|q| promatica_core::find_schematic(&dir, q).ok())
        .filter_map(|f| std::fs::metadata(dir.join(f)).ok())
        .map(|m| m.len())
        .sum();
    let (tx, done) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("promatica-load".into())
        .spawn(move || {
            let _ = tx.send(queries.iter().map(|q| read_file(&dir, q)).collect());
        })
        .map_err(|e| format!("can't start reading: {e}"))?;
    let job = LoadJob { files, then, world: world_key(game), started: std::time::Instant::now(), bytes, done };
    match job.done.recv_timeout(LOAD_WAIT) {
        Ok(read) => finish_load(game, job, read),
        Err(RecvTimeoutError::Disconnected) => Err(format!("reading {} failed", job.label())),
        Err(RecvTimeoutError::Timeout) => {
            let line = format!("Loading {} ({}) in the background...", job.label(), human_size(job.bytes));
            with(|s| s.loads.push(job));
            Ok(vec![line])
        }
    }
}

/// Places the files of a job that are in; jobs for a world since left are dropped.
fn load_step(game: &mut Game<'_>) {
    use std::sync::mpsc::TryRecvError;
    let jobs = with(|s| std::mem::take(&mut s.loads));
    let mut left = Vec::new();
    for job in jobs {
        let lines = match job.done.try_recv() {
            Err(TryRecvError::Empty) => {
                left.push(job);
                continue;
            }
            Ok(_) if job.world != world_key(game) => continue,
            Ok(read) => finish_load(game, job, read),
            Err(TryRecvError::Disconnected) => Err(format!("reading {} failed", job.label())),
        };
        let lines = lines.unwrap_or_else(|e| vec![format!("Error: {e}")]);
        if let Some(first) = lines.first() {
            with(|s| s.ui_message = first.clone());
        }
        for l in &lines {
            say(game, l);
        }
    }
    with(|s| {
        left.append(&mut s.loads);
        s.loads = left;
    });
}

/// The HUD's line for each job still reading.
fn load_lines(s: &State) -> Vec<(String, u32)> {
    s.loads
        .iter()
        .map(|j| {
            let secs = j.started.elapsed().as_secs();
            (format!("Loading {} ({}) · {secs} s", j.label(), human_size(j.bytes)), 0xFFFF55)
        })
        .collect()
}

/// What a job's files become: placements, and the lines that tell of them.
fn finish_load(game: &mut Game<'_>, job: LoadJob, read: Vec<Result<ReadFile, String>>) -> Result<Vec<String>, String> {
    match job.then {
        Then::Load => {
            let (read, at) = first_read(&job.files, read)?;
            let (slot, mut msg) = place(game, read, at);
            let count = with(|s| {
                s.slots.push(slot);
                s.selected = s.slots.len() - 1;
                s.layer = LayerMode::All;
                s.moving = false;
                s.slots.len()
            });
            save_placements(game);
            if count > 1 {
                msg += &format!(" · placement {count} of {count}");
            }
            if !with(|s| std::mem::replace(&mut s.credited, true)) {
                say(
                    game,
                    "Promatica is based on Litematica by masa (maruohon) & Sakura-Ryoko and the Litematica Printer by aleksilassila. /hm credits",
                );
            }
            Ok(vec![msg])
        }
        Then::Reload => {
            let (read, at) = first_read(&job.files, read)?;
            // Still loaded there: read again from its file (after it was changed), else added.
            let (slot, msg) = place(game, read, at);
            let replaced = with(|s| {
                let same = s.slots.iter().position(|x| x.placement.file_name == slot.placement.file_name && x.placement.origin == at.0);
                match same {
                    Some(i) => {
                        let (shown, locked) = (s.slots[i].shown, s.slots[i].locked);
                        s.slots[i] = Slot { shown, locked, ..slot };
                        s.selected = i;
                    }
                    None => {
                        s.slots.push(slot);
                        s.selected = s.slots.len() - 1;
                    }
                }
                same.is_some()
            });
            save_placements(game);
            Ok(vec![if replaced { msg.replacen("Loaded", "Reloaded", 1) } else { msg }])
        }
        Then::Restore { flags, selected } => {
            let mut lines = Vec::new();
            let mut restored = Vec::new();
            for (((file, at), read), (shown, locked)) in job.files.iter().zip(read).zip(flags) {
                match read {
                    Ok(read) => {
                        let (mut slot, _) = place(game, read, *at);
                        slot.shown = shown;
                        slot.locked = locked;
                        restored.push(format!("{file} at {} {} {}", at.0[0], at.0[1], at.0[2]));
                        with(|s| s.slots.push(slot));
                    }
                    Err(e) => lines.push(format!("Couldn't restore {file}: {e}")),
                }
            }
            with(|s| s.selected = selected.min(s.slots.len().saturating_sub(1)));
            match restored.as_slice() {
                [] => {}
                [one] => lines.push(format!("Restored {one} (/hm unload removes it)")),
                many => lines.push(format!("Restored {} placements: {} (/hm placements)", many.len(), many.join(", "))),
            }
            Ok(lines)
        }
    }
}

fn first_read(files: &[(String, At)], read: Vec<Result<ReadFile, String>>) -> Result<(ReadFile, At), String> {
    let at = files.first().ok_or("no file")?.1;
    Ok((read.into_iter().next().ok_or("no file")??, at))
}

/// Places a file that was read: a new slot, not yet added, and the line telling what was loaded.
fn place(game: &mut Game<'_>, read: ReadFile, at: At) -> (Slot, String) {
    let ReadFile { file, schematic, format, from_version, upgraded, histograms } = read;
    let (origin, rotation, mirror) = at;
    let mut placement = Placement::new(file, schematic, origin);
    placement.set_rotation(rotation);
    placement.set_mirror(mirror);
    let (lo, hi) = placement.bounds();
    let size = [0, 1, 2].map(|i| hi[i] - lo[i] + 1);
    let (slot, unknown) = with(|s| {
        load_settings(game, s);
        let totals = count_totals(game, s, &placement, &histograms);
        let mut unknown: Vec<String> = Vec::new();
        for st in placement.palettes().iter().flatten() {
            if !st.is_air() && !st.is_void() && wanted_id(game, s, st).is_none() && !unknown.contains(&st.name) {
                unknown.push(st.name.clone());
            }
        }
        s.next_id += 1;
        (Slot { id: s.next_id, placement, shown: true, locked: false, totals }, unknown)
    });
    let mut msg = format!(
        "Loaded {} ({}x{}x{}, {} regions) at {} {} {}",
        display_name(&slot.placement),
        size[0],
        size[1],
        size[2],
        slot.placement.schematic.regions.len(),
        origin[0],
        origin[1],
        origin[2]
    );
    if format != Format::Litematic {
        msg.push_str(&format!(" · {}", format.name()));
    }
    if upgraded > 0 {
        msg.push_str(&format!(" · {upgraded} block states updated from data version {from_version}"));
    }
    if !unknown.is_empty() {
        msg += &format!(". Unknown blocks (skipped): {}", unknown.join(", "));
    }
    (slot, msg)
}

/// Loads a schematic as another placement (at the player's feet without `at`) and selects it.
fn load(game: &mut Game<'_>, query: &str, at: Option<At>) -> Result<Vec<String>, String> {
    let at = match at {
        Some(a) => a,
        None => (feet(game).ok_or("not in a world")?, Rotation::None, Mirror::None),
    };
    start_load(game, vec![(query.to_string(), at)], Then::Load)
}

// ---------------------------------------------------------------- commands

pub(super) fn commands(commands: &mut Vec<&'static str>) {
    commands.push("promatica");
    commands.push("hm");
}

/// The tree the chat colours and completes with: as the game builds it, with this mod's commands
/// spelled out, so the chat offers each word as it is typed. Every word still leads to `command`,
/// which reads the line itself.
pub(super) fn chat_tree(game: &mut Game<'_>, base: Option<&horizon_client::command::Dispatcher>) -> horizon_client::command::Dispatcher {
    let mut names = Vec::new();
    game.native_commands(&mut names);
    let base = base.unwrap_or_else(|| horizon_client::command::vanilla::game_dispatcher());
    let mut tree = super::chat::with_native_commands(base, &names);
    for name in ["promatica", "hm"] {
        if let Some(root) = tree.find(name) {
            add_words(&mut tree, root);
        }
    }
    let (files, slots) = with(|s| s.completions);
    set_completions(&mut tree, files, slots);
    tree
}

/// Every subcommand with the words it takes, and the name shown for what it takes besides.
const WORDS: &[(&str, &[&str], Option<&str>)] = &[
    ("help", &[], None),
    ("list", &[], None),
    ("load", &[], Some("file")),
    ("reload", &[], None),
    ("unload", &["all"], Some("placement")),
    ("placements", &[], None),
    ("select", &["next", "prev"], Some("placement")),
    ("show", &["on", "off"], None),
    ("hide", &[], None),
    ("lock", &["on", "off"], None),
    ("unlock", &[], None),
    ("origin", &["here"], Some("x y z")),
    ("move", &[], Some("dx dy dz")),
    ("rotate", &["cw", "ccw", "180", "none"], None),
    ("mirror", &["none", "x", "z"], None),
    ("pos1", &["here"], Some("x y z")),
    ("pos2", &["here"], Some("x y z")),
    ("area", &["clear"], None),
    ("save", &["status", "cancel"], Some("name")),
    ("tool", &["on", "off", "mode"], Some("item")),
    ("layer", &["all", "up", "down", "here", "above", "below", "range"], Some("y")),
    ("render", &["on", "off"], None),
    ("through", &["on", "off"], None),
    ("highlight", &["missing", "wrong", "item", "off"], None),
    ("mode", &["ghost", "boxes"], None),
    ("radius", &["view"], Some("blocks")),
    ("faces", &["300k", "1m", "2m", "5m", "10m", "none"], Some("number")),
    ("fluids", &["on", "off"], None),
    ("info", &[], None),
    ("materials", &["on", "off", "copy"], None),
    ("printer", &["on", "off", "speed", "reach", "air"], None),
    ("easyplace", &["on", "off", "every", "air"], None),
    ("folder", &[], None),
    ("credits", &[], None),
];

/// What a word takes in turn: (subcommand, word, its words, the name shown for what it takes besides).
const SUBWORDS: &[(&str, &str, &[&str], Option<&str>)] = &[
    ("tool", "mode", &["area", "placement"], None),
    ("layer", "above", &[], Some("y")),
    ("layer", "below", &[], Some("y")),
    ("layer", "range", &[], Some("a b")),
    ("printer", "speed", &[], Some("ticks")),
    ("printer", "reach", &[], Some("blocks")),
    ("printer", "air", &["on", "off"], None),
    ("easyplace", "every", &[], Some("ticks")),
    ("easyplace", "air", &["on", "off"], None),
];

/// A word under `parent` that a command may end with.
fn add_word(tree: &mut horizon_client::command::Dispatcher, parent: usize, word: &str) -> usize {
    let node = tree.literal(parent, word);
    tree.executes(node, horizon_client::command::MOD_COMMAND)
}

/// Free text under `parent`, to the end of the line (the mod reads it).
fn add_rest(tree: &mut horizon_client::command::Dispatcher, parent: usize, name: &str) -> usize {
    let node = tree.argument(parent, name, horizon_client::command::ArgType::Greedy);
    tree.executes(node, horizon_client::command::MOD_COMMAND)
}

fn add_words(tree: &mut horizon_client::command::Dispatcher, root: usize) {
    for &(sub, words, rest) in WORDS {
        let node = add_word(tree, root, sub);
        for word in words {
            add_word(tree, node, word);
        }
        if let Some(name) = rest {
            add_rest(tree, node, name);
        }
        for &(of, word, words, rest) in SUBWORDS {
            if of == sub {
                let node = add_word(tree, node, word);
                for word in words {
                    add_word(tree, node, word);
                }
                if let Some(name) = rest {
                    add_rest(tree, node, name);
                }
            }
        }
    }
    if let Some(item) = child(tree, root, "tool").and_then(|tool| child(tree, tool, "item")) {
        tree.suggests(item, horizon_client::command::Suggests::Fixed(&TOOL_ITEMS));
    }
}

/// The child of `node` called `name`.
fn child(tree: &horizon_client::command::Dispatcher, node: usize, name: &str) -> Option<usize> {
    tree.nodes[node].children.iter().copied().find(|&c| tree.nodes[c].name() == name)
}

/// Offers the schematic files after `load`, and the placements after `select` and `unload`.
fn set_completions(tree: &mut horizon_client::command::Dispatcher, files: &'static [&'static str], slots: &'static [&'static str]) {
    use horizon_client::command::Suggests;
    let targets: [(&str, &str, &'static [&'static str]); 3] = [("load", "file", files), ("select", "placement", slots), ("unload", "placement", slots)];
    for name in ["promatica", "hm"] {
        let Some(root) = tree.find(name) else { continue };
        for (sub, arg, list) in targets {
            if let Some(node) = child(tree, root, sub).and_then(|n| child(tree, n, arg)) {
                tree.suggests(node, Suggests::Fixed(list));
            }
        }
    }
}

/// While the chat is open, keeps the names it offers current. The tree only takes `'static` lists,
/// so a list is leaked when it changes: a few names, and only when files or placements change.
fn completions_step(game: &mut Game<'_>) {
    if game.ui.chat_screen.is_none() || game.ticks % 20 != 0 {
        return;
    }
    let files = schematics_dir().map(|dir| promatica_core::list_schematics(&dir)).unwrap_or_default();
    let slots: Vec<String> = with(|s| s.slots.iter().map(|x| display_name(&x.placement)).collect());
    let same = |old: &[&str], new: &[String]| old.len() == new.len() && old.iter().zip(new).all(|(a, b)| *a == b);
    let leak = |new: Vec<String>| -> &'static [&'static str] {
        Vec::leak(new.into_iter().map(|n| &*String::leak(n)).collect::<Vec<&'static str>>())
    };
    let (old_files, old_slots) = with(|s| s.completions);
    if same(old_files, &files) && same(old_slots, &slots) {
        return;
    }
    let files = if same(old_files, &files) { old_files } else { leak(files) };
    let slots = if same(old_slots, &slots) { old_slots } else { leak(slots) };
    with(|s| s.completions = (files, slots));
    if let Some(remote) = game.remote.as_mut() {
        set_completions(&mut remote.chat_tree, files, slots);
    }
}

fn say(game: &mut Game<'_>, text: &str) {
    game.native_chat(&format!("[Promatica] {text}"));
}

const HELP: &[&str] = &[
    "/promatica list | load <name> | unload [all] | reload | folder | info",
    "/promatica placements | select <n>|next|prev | show [on|off] | hide | lock [on|off] | unlock",
    "/promatica origin here | origin <x> <y> <z> | move <dx> <dy> <dz> | rotate cw|ccw|180|none | mirror none|x|z",
    "/promatica pos1|pos2 [here | <x> <y> <z>] (default: the block you look at) | area [clear] | save <name> [overwrite] | save cancel|status",
    "/promatica tool on|off|<item> | tool mode area|placement: the selection tool (a stick)",
    "/promatica layer all | <y> | above <y> | below <y> | range <a> <b> | up | down | here",
    "/promatica render on|off | through on|off | mode ghost|boxes | verify | materials [on|off] | materials copy",
    "/promatica radius <blocks>|view | faces <n>|none | fluids on|off",
    "/promatica highlight missing|wrong [on|off] | highlight item <item>|all | highlight off",
    "/promatica printer on|off | air on|off | speed <ticks> | reach <blocks>",
    "/promatica easyplace on|off | easyplace every <ticks> | easyplace air on|off: hold right click to place the block the schematic wants",
    "/promatica credits: who made the original Litematica and its printer",
    "(/hm works as a short form)",
];

/// Runs `/promatica …`; true when the command was ours.
pub(super) fn command(game: &mut Game<'_>, line: &str) -> bool {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("promatica" | "hm") => {}
        _ => return false,
    }
    let args: Vec<&str> = words.collect();
    let result = run(game, &args);
    match result {
        Ok(lines) => {
            for l in lines {
                say(game, &l);
            }
        }
        Err(e) => say(game, &format!("Error: {e}")),
    }
    true
}

fn int(s: Option<&&str>, what: &str) -> Result<i32, String> {
    s.ok_or_else(|| format!("missing {what}"))?.parse::<i32>().map_err(|_| format!("{what} must be a number"))
}

fn on_off(s: Option<&&str>, current: bool) -> Result<bool, String> {
    match s.copied() {
        None | Some("toggle") => Ok(!current),
        Some("on" | "true" | "1") => Ok(true),
        Some("off" | "false" | "0") => Ok(false),
        Some(v) => Err(format!("expected on or off, not {v}")),
    }
}

fn need_placement(s: &State) -> Result<(), String> {
    if s.slots.is_empty() { Err("no schematic loaded (/promatica load <name>)".into()) } else { Ok(()) }
}

/// The selected placement, to change it.
fn selected_mut(s: &mut State) -> Result<&mut Placement, String> {
    need_placement(s)?;
    if let Some(slot) = s.sel()
        && slot.locked
    {
        return Err(format!("{} is locked (/hm unlock)", display_name(&slot.placement)));
    }
    s.placement_mut().ok_or_else(|| "no placement selected".into())
}

/// A placement by its number in `/hm placements` (from 1), else by (the start of) its name.
fn find_slot(s: &State, query: &str) -> Result<usize, String> {
    if let Ok(n) = query.parse::<usize>() {
        return (1..=s.slots.len()).contains(&n).then(|| n - 1).ok_or_else(|| format!("no placement {n} (there are {})", s.slots.len()));
    }
    let q = query.to_lowercase();
    let names: Vec<(usize, String, String)> =
        s.slots.iter().enumerate().map(|(i, x)| (i, display_name(&x.placement).to_lowercase(), x.placement.file_name.to_lowercase())).collect();
    if let Some((i, ..)) = names.iter().find(|(_, n, f)| *n == q || *f == q) {
        return Ok(*i);
    }
    let hits: Vec<usize> = names.iter().filter(|(_, n, f)| n.starts_with(&q) || f.starts_with(&q)).map(|(i, ..)| *i).collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("no placement \"{query}\" (/hm placements)")),
        _ => Err(format!("\"{query}\" matches {} placements: use its number (/hm placements)", hits.len())),
    }
}

/// One line per placement for `/hm placements`.
fn placement_lines(s: &State) -> Vec<String> {
    s.slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let p = &slot.placement;
            format!(
                "{}{} {} at {} {} {}{}",
                if i == s.selected { "> " } else { "  " },
                i + 1,
                display_name(p),
                p.origin[0],
                p.origin[1],
                p.origin[2],
                match (slot.shown, slot.locked) {
                    (true, false) => "",
                    (false, false) => " (hidden)",
                    (true, true) => " (locked)",
                    (false, true) => " (hidden, locked)",
                }
            )
        })
        .collect()
}

/// The block the player looks at (up to 64 blocks away), else where they stand.
fn target_block(game: &Game<'_>) -> Option<[i32; 3]> {
    let looked = game.aim().and_then(|(origin, dir)| game.clip_ray(origin, dir, 64.0)).map(|hit| hit.pos);
    looked.or_else(|| feet(game))
}

fn run(game: &mut Game<'_>, args: &[&str]) -> Result<Vec<String>, String> {
    with(|s| load_settings(game, s));
    let sub = args.first().copied().unwrap_or("help");
    let rest = &args[args.len().min(1)..];
    let changed = |game: &mut Game<'_>| {
        with(|s| s.mesh_key = None);
        save_placements(game);
    };
    match sub {
        "help" | "?" => Ok(HELP.iter().map(|s| s.to_string()).collect()),
        "credits" | "about" => Ok(CREDITS.iter().map(|s| s.to_string()).collect()),
        "folder" => Ok(vec![format!("Schematics folder: {}", schematics_dir()?.display())]),
        "list" => {
            let dir = schematics_dir()?;
            let names = promatica_core::list_schematics(&dir);
            if names.is_empty() {
                return Ok(vec![format!("No schematic files (.litematic, .schem, .schematic, .nbt) in {}", dir.display())]);
            }
            Ok(vec![format!("{} schematics: {}", names.len(), names.join(", "))])
        }
        "load" => {
            if rest.is_empty() {
                return Err("usage: /promatica load <name>".into());
            }
            load(game, &rest.join(" "), None)
        }
        "reload" => {
            let last = game.native_setting(ID, "last").ok_or("nothing loaded before")?;
            let (file, at) = parse_saved(&last).ok_or("the saved placement is unreadable")?;
            start_load(game, vec![(file, at)], Then::Reload)
        }
        "unload" | "remove" => {
            // Gone from this world too: it doesn't come back on the next join.
            let msg = if rest.first() == Some(&"all") {
                let n = with(|s| {
                    let n = s.slots.len();
                    s.drop_all();
                    n
                });
                format!("Unloaded {n} placement{}.", if n == 1 { "" } else { "s" })
            } else {
                let (name, left) = with(|s| {
                    need_placement(s)?;
                    let i = if rest.is_empty() { s.selected } else { find_slot(s, &rest.join(" "))? };
                    let slot = s.slots.remove(i);
                    if s.selected > i || s.selected >= s.slots.len() {
                        s.selected = s.selected.saturating_sub(1);
                    }
                    if s.slots.is_empty() {
                        s.drop_all();
                    } else {
                        s.moving = false;
                    }
                    Ok::<_, String>((display_name(&slot.placement), s.slots.len()))
                })?;
                if left == 0 { format!("Unloaded {name}.") } else { format!("Unloaded {name}; {left} left (/hm placements).") }
            };
            if with(|s| s.slots.is_empty()) {
                bridge::clear();
            }
            save_placements(game);
            Ok(vec![msg])
        }
        "placements" | "pl" => with(|s| {
            need_placement(s)?;
            let mut out = vec![format!("{} placements (> selected):", s.slots.len())];
            out.extend(placement_lines(s));
            Ok(out)
        }),
        "select" | "sel" => {
            let line = with(|s| {
                need_placement(s)?;
                let n = s.slots.len();
                s.selected = match rest.first().copied() {
                    None | Some("next") => (s.selected + 1) % n,
                    Some("prev" | "previous") => (s.selected + n - 1) % n,
                    Some(_) => find_slot(s, &rest.join(" "))?,
                };
                s.moving = false;
                let p = s.placement().unwrap();
                Ok::<_, String>(format!("Selected {} of {}: {}", s.selected + 1, n, display_name(p)))
            })?;
            save_placements(game);
            Ok(vec![line])
        }
        "lock" | "unlock" => {
            let line = with(|s| {
                need_placement(s)?;
                let slot = &mut s.slots[s.selected];
                slot.locked = if sub == "unlock" { false } else { on_off(rest.first(), slot.locked)? };
                let locked = slot.locked;
                if locked {
                    s.moving = false;
                }
                let name = display_name(&s.slots[s.selected].placement);
                Ok::<_, String>(if locked {
                    format!("{name} locked: it can't be moved, turned or mirrored (/hm unlock)")
                } else {
                    format!("{name} unlocked")
                })
            })?;
            save_placements(game);
            Ok(vec![line])
        }
        "show" | "hide" => {
            let line = with(|s| {
                need_placement(s)?;
                let slot = &mut s.slots[s.selected];
                slot.shown = if sub == "hide" { false } else { on_off(rest.first(), slot.shown)? };
                if !slot.shown {
                    s.moving = false;
                }
                let slot = &s.slots[s.selected];
                Ok::<_, String>(format!(
                    "{} {}",
                    display_name(&slot.placement),
                    if slot.shown { "shown" } else { "hidden (stays loaded; /hm show brings it back)" }
                ))
            })?;
            save_placements(game);
            Ok(vec![line])
        }
        "pos1" | "pos2" | "corner1" | "corner2" => {
            let at = match rest.first().copied() {
                None => target_block(game).ok_or("not in a world")?,
                Some("here") => feet(game).ok_or("not in a world")?,
                _ => [int(rest.first(), "x")?, int(rest.get(1), "y")?, int(rest.get(2), "z")?],
            };
            let i = usize::from(sub.ends_with('2'));
            let size = with(|s| {
                s.area[i] = Some(at);
                s.area_box().map(|(lo, hi)| [0, 1, 2].map(|k| hi[k] - lo[k] + 1))
            });
            let mut line = format!("Corner {} at {} {} {}", i + 1, at[0], at[1], at[2]);
            if let Some(d) = size {
                line += &format!(" · area {} x {} x {} (/hm save <name>)", d[0], d[1], d[2]);
            }
            Ok(vec![line])
        }
        "area" | "selection" => {
            if matches!(rest.first().copied(), Some("clear" | "none" | "off")) {
                with(|s| s.area = [None; 2]);
                return Ok(vec!["Area selection cleared.".into()]);
            }
            let (area, size) = with(|s| (s.area, s.area_box().map(|(lo, hi)| [0, 1, 2].map(|k| hi[k] - lo[k] + 1))));
            let corner = |c: Option<[i32; 3]>| c.map_or("not set".to_string(), |c| format!("{} {} {}", c[0], c[1], c[2]));
            let mut line = format!("Area: corner 1 {}, corner 2 {}", corner(area[0]), corner(area[1]));
            if let Some(d) = size {
                line += &format!(" · {} x {} x {} = {} blocks", d[0], d[1], d[2], d.iter().map(|&v| i64::from(v)).product::<i64>());
            }
            Ok(vec![line])
        }
        "tool" => {
            let line = with(|s| -> Result<String, String> {
                let st = &mut s.settings;
                match rest.first().copied() {
                    None => {}
                    Some("mode") => {
                        st.tool_placement = match rest.get(1).copied() {
                            Some("area" | "selection") => false,
                            Some("placement" | "placements" | "move") => true,
                            None | Some("toggle") => !st.tool_placement,
                            Some(v) => return Err(format!("tool mode area or placement, not {v}")),
                        };
                        st.tool = true;
                        return Ok(if st.tool_placement {
                            "Selection tool: placement mode · left click selects, right click moves it there, Alt+wheel moves, Ctrl+wheel turns"
                                .into()
                        } else {
                            "Selection tool: area mode · left click corner 1, right click corner 2".into()
                        });
                    }
                    Some("on" | "off" | "toggle") => st.tool = on_off(rest.first(), st.tool)?,
                    Some(item) => {
                        let item = item.to_ascii_lowercase();
                        st.tool_item = if item.contains(':') { item } else { format!("minecraft:{item}") };
                        st.tool = true;
                    }
                }
                Ok(if st.tool {
                    format!("Selection tool ON: {} in hand · left click corner 1, right click corner 2", short(&st.tool_item))
                } else {
                    "Selection tool OFF (/hm pos1 and /hm pos2 still set the corners)".into()
                })
            })?;
            let settings = with(|s| s.settings.clone());
            save_settings(game, &settings);
            with(|s| s.mesh.area = None);
            Ok(vec![line])
        }
        "save" => {
            match rest {
                ["cancel" | "stop"] => return Ok(vec![cancel_save()?]),
                ["status"] => {
                    let lines = with(|s| save_lines(game, s)).map(|(l, _)| l.into_iter().map(|(t, _)| t).collect());
                    return Ok(lines.unwrap_or_else(|| vec!["No save under way.".into()]));
                }
                _ => {}
            }
            let overwrite = rest.last() == Some(&"overwrite") && rest.len() > 1;
            let name = rest[..rest.len() - usize::from(overwrite)].join(" ");
            if name.is_empty() {
                return Err("usage: /promatica save <name> [overwrite] | save cancel | save status".into());
            }
            Ok(vec![start_save(game, &name, overwrite)?])
        }
        "info" | "verify" => with(|s| {
            need_placement(s)?;
            let p = s.placement().unwrap();
            let (lo, hi) = p.bounds();
            let mut out = vec![format!(
                "{} ({} of {}) — origin {} {} {}, rotation {}, mirror {}, box {:?}..{:?}, layer {}",
                p.file_name,
                s.selected + 1,
                s.slots.len(),
                p.origin[0],
                p.origin[1],
                p.origin[2],
                p.rotation.name(),
                p.mirror.name(),
                lo,
                hi,
                s.layer.describe()
            )];
            match s.tally() {
                Some(t) => out.push(format!(
                    "{:.1}% done: {} correct, {} missing, {} wrong block, {} wrong state, {} extra{}",
                    t.percent(),
                    t.correct,
                    t.missing,
                    t.wrong_block,
                    t.wrong_state,
                    t.extra,
                    if s.unloaded > 0 { format!(" ({} cells in unloaded chunks)", s.unloaded) } else { String::new() }
                )),
                None => out.push("Verifier: still scanning…".into()),
            }
            Ok(out)
        }),
        "origin" => {
            let at = match rest.first().copied() {
                Some("here") | None => feet(game).ok_or("not in a world")?,
                _ => [int(rest.first(), "x")?, int(rest.get(1), "y")?, int(rest.get(2), "z")?],
            };
            with(|s| {
                selected_mut(s)?.set_origin(at);
                Ok::<_, String>(())
            })?;
            changed(game);
            Ok(vec![format!("Origin set to {} {} {}", at[0], at[1], at[2])])
        }
        "move" | "nudge" => {
            let d = [int(rest.first(), "dx")?, int(rest.get(1), "dy")?, int(rest.get(2), "dz")?];
            let o = with(|s| {
                let p = selected_mut(s)?;
                p.nudge(d);
                Ok::<_, String>(p.origin)
            })?;
            changed(game);
            Ok(vec![format!("Origin now {} {} {}", o[0], o[1], o[2])])
        }
        "rotate" => {
            let r = with(|s| {
                let p = selected_mut(s)?;
                let r = match rest.first().copied() {
                    None | Some("cw") => p.rotation.then(Rotation::Clockwise90),
                    Some("ccw") => p.rotation.then(Rotation::CounterClockwise90),
                    Some(v) => Rotation::parse(v).ok_or("rotate cw, ccw, 180 or none")?,
                };
                p.set_rotation(r);
                Ok::<_, String>(r)
            })?;
            changed(game);
            Ok(vec![format!("Rotation: {}", r.name())])
        }
        "mirror" => {
            let m = Mirror::parse(rest.first().copied().unwrap_or("none")).ok_or("mirror none, x or z")?;
            with(|s| {
                selected_mut(s)?.set_mirror(m);
                Ok::<_, String>(())
            })?;
            changed(game);
            Ok(vec![format!("Mirror: {}", m.name())])
        }
        "layer" => {
            let feet_y = feet(game).map_or(64, |f| f[1]);
            let l = with(|s| {
                need_placement(s)?;
                let base = s.placement().unwrap().bounds().0[1];
                let current = match s.layer {
                    LayerMode::All => base,
                    LayerMode::Single(y) | LayerMode::Above(y) | LayerMode::Below(y) | LayerMode::Range(y, _) => y,
                };
                s.layer = match rest.first().copied() {
                    None | Some("all") => LayerMode::All,
                    Some("up") => match s.layer {
                        LayerMode::All => LayerMode::Single(base),
                        l => l.shifted(1),
                    },
                    Some("down") => match s.layer {
                        LayerMode::All => LayerMode::Single(base),
                        l => l.shifted(-1),
                    },
                    Some("here") => LayerMode::Single(feet_y),
                    Some("above") => LayerMode::Above(rest.get(1).map_or(Ok(current), |_| int(rest.get(1), "y"))?),
                    Some("below") => LayerMode::Below(rest.get(1).map_or(Ok(current), |_| int(rest.get(1), "y"))?),
                    Some("range") => LayerMode::Range(int(rest.get(1), "min y")?, int(rest.get(2), "max y")?),
                    Some(_) => LayerMode::Single(int(rest.first(), "y")?),
                };
                s.mesh_key = None;
                Ok::<_, String>(s.layer)
            })?;
            Ok(vec![format!("Layers: {}", l.describe())])
        }
        "render" | "mode" | "through" | "xray" | "radius" | "faces" | "fluids" => {
            let view = game.settings.render_distance as i32 * 16;
            let msg = with(|s| {
                let st = &mut s.settings;
                let msg = match sub {
                    "radius" => {
                        match rest.first().copied() {
                            Some("view") => st.radius_view = true,
                            Some(v) => {
                                st.radius = v.parse::<i32>().map_err(|_| format!("radius in blocks or view, not {v}"))?.max(16);
                                st.radius_view = false;
                            }
                            None => {}
                        }
                        if st.radius_view {
                            format!("Render radius: the view distance ({view} blocks)")
                        } else {
                            format!("Render radius: {} blocks", st.radius)
                        }
                    }
                    "faces" => {
                        if let Some(v) = rest.first() {
                            st.max_faces = parse_faces(v).ok_or_else(|| format!("a number of faces (300k, 2m...) or none, not {v}"))?;
                        }
                        let mem = if st.max_faces == 0 { String::new() } else { format!(" (up to ~{} of video memory)", mib(st.max_faces * FACE_BYTES)) };
                        format!("Face limit: {}{mem}", faces_text(st.max_faces))
                    }
                    "fluids" => {
                        st.hide_fluids = !on_off(rest.first(), !st.hide_fluids)?;
                        format!("Water and lava {}", if st.hide_fluids { "hidden" } else { "shown" })
                    }
                    "render" => {
                        st.render = on_off(rest.first(), st.render)?;
                        format!("Rendering {}", if st.render { "on" } else { "off" })
                    }
                    "through" | "xray" => {
                        st.through = on_off(rest.first(), st.through)?;
                        format!("See through blocks {}", if st.through { "on" } else { "off" })
                    }
                    _ => {
                        st.mode = match rest.first().copied() {
                            Some("ghost" | "ghosts") => Mode::Ghost,
                            Some("box" | "boxes") => Mode::Boxes,
                            None => if st.mode == Mode::Ghost { Mode::Boxes } else { Mode::Ghost },
                            Some(v) => return Err(format!("mode ghost or boxes, not {v}")),
                        };
                        format!("Mode: {}", if st.mode == Mode::Ghost { "ghost blocks" } else { "boxes" })
                    }
                };
                s.mesh_key = None;
                Ok::<_, String>(msg)
            })?;
            with(|s| save_settings(game, &s.settings));
            Ok(vec![msg])
        }
        "highlight" | "hl" => {
            let list = material_list(game);
            let line = with(|s| -> Result<String, String> {
                match rest.first().copied() {
                    None => {}
                    Some("off") => {
                        s.settings.highlight_missing = false;
                        s.settings.highlight_wrong = false;
                        s.highlight_item = None;
                    }
                    Some("missing") => s.settings.highlight_missing = on_off(rest.get(1), s.settings.highlight_missing)?,
                    Some("wrong") => s.settings.highlight_wrong = on_off(rest.get(1), s.settings.highlight_wrong)?,
                    Some("item") => match rest[1..].join("_").to_lowercase().as_str() {
                        "" | "all" | "none" => s.highlight_item = None,
                        name => {
                            let item = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
                            if !list.iter().any(|m| m.item == item) {
                                return Err(format!("{name} isn't in the selected schematic's material list"));
                            }
                            s.highlight_item = Some(item);
                            if !s.settings.highlight_wrong {
                                s.settings.highlight_missing = true;
                            }
                        }
                    },
                    Some(v) => return Err(format!("highlight missing, wrong, item or off, not {v}")),
                }
                save_settings(game, &s.settings);
                Ok(highlight_text(s))
            })?;
            Ok(vec![line])
        }
        "materials" | "mats" => {
            if rest.first() == Some(&"copy") {
                return copy_materials(game);
            }
            let line = with(|s| -> Result<String, String> {
                s.settings.materials_hud = on_off(rest.first(), s.settings.materials_hud)?;
                save_settings(game, &s.settings);
                Ok(format!("Material list in the HUD: {}", if s.settings.materials_hud { "on" } else { "off" }))
            })?;
            Ok(vec![line])
        }
        "printer" | "print" => {
            let msg = with(|s| {
                let st = &mut s.settings;
                let msg = match rest.first().copied() {
                    Some("speed") => {
                        st.printer.interval = int(rest.get(1), "ticks")?.clamp(1, 20) as u32;
                        format!("Printer: one block every {} ticks", st.printer.interval)
                    }
                    Some("reach") => {
                        let r: f64 = rest.get(1).ok_or("missing reach")?.parse().map_err(|_| "reach must be a number")?;
                        st.printer.reach = r.clamp(1.0, 6.0);
                        format!("Printer reach: {:.1}", st.printer.reach)
                    }
                    Some("air") => {
                        st.printer.air_place = on_off(rest.get(1), st.printer.air_place)?;
                        format!(
                            "Printer air place {}",
                            if st.printer.air_place { "ON — places without a neighbouring block" } else { "OFF" }
                        )
                    }
                    v => {
                        st.printer_on = on_off(v.as_ref(), st.printer_on)?;
                        s.recent.clear();
                        format!(
                            "Printer {}",
                            if st.printer_on { "ON — places missing blocks within reach" } else { "OFF" }
                        )
                    }
                };
                Ok::<_, String>(msg)
            })?;
            with(|s| save_settings(game, &s.settings));
            Ok(vec![msg])
        }
        "easyplace" | "easy" => {
            let msg = with(|s| {
                let st = &mut s.settings;
                let msg = match rest.first().copied() {
                    Some("every") => {
                        st.easy_interval = int(rest.get(1), "ticks")?.clamp(1, 10) as u32;
                        format!("Easy place: one block every {} ticks", st.easy_interval)
                    }
                    Some("air") => {
                        st.easy_air = on_off(rest.get(1), st.easy_air)?;
                        format!(
                            "Easy air place {}",
                            if st.easy_air { "ON — ghost blocks need no neighbouring block" } else { "OFF" }
                        )
                    }
                    v => {
                        st.easy_place = on_off(v.as_ref(), st.easy_place)?;
                        format!(
                            "Easy place {}",
                            if st.easy_place { "ON — hold right click to place what the schematic wants" } else { "OFF" }
                        )
                    }
                };
                Ok::<_, String>(msg)
            })?;
            with(|s| save_settings(game, &s.settings));
            Ok(vec![msg])
        }
        other => Err(format!("unknown subcommand {other}; /promatica help")),
    }
}

fn short(item: &str) -> &str {
    item.strip_prefix("minecraft:").unwrap_or(item)
}

/// A key's name as the Keys page shows it: `key.keyboard.page.up` → `PAGE UP`, `key.mouse.4` → `MOUSE 4`.
fn key_label(name: &str) -> String {
    let bare = name.strip_prefix("key.keyboard.").or_else(|| name.strip_prefix("key.")).unwrap_or(name);
    if bare.is_empty() {
        return "not bound".into();
    }
    bare.replace('.', " ").to_uppercase()
}

// ---------------------------------------------------------------- keys

/// Loads the binds once: a key the player set on the Keys page, else the one Options > Mods
/// kept for it, else the default.
fn ensure_binds(game: &Game<'_>, s: &mut State) {
    if s.binds.is_some() {
        return;
    }
    s.binds = Some(
        BINDS
            .iter()
            .map(|(id, _, default)| {
                let key = game.native_setting(ID, &bind_file(id)).unwrap_or_else(|| game.native_mod_key(ID, id, default));
                (id.to_string(), key)
            })
            .collect(),
    );
}

fn bind_file(id: &str) -> String {
    format!("key_{id}")
}

/// The key bound to `id` ("" when none).
fn key_of(game: &Game<'_>, id: &str) -> String {
    with(|s| {
        ensure_binds(game, s);
        s.binds.as_ref().and_then(|b| b.iter().find(|(i, _)| i == id)).map(|(_, k)| k.clone()).unwrap_or_default()
    })
}

/// Binds `id` to `key` ("" unbinds it), taking the key from any other action first, and saves it.
fn set_bind(game: &Game<'_>, id: &str, key: &str) {
    let mut changed = Vec::new();
    with(|s| {
        ensure_binds(game, s);
        let Some(binds) = s.binds.as_mut() else { return };
        for (i, k) in binds.iter_mut() {
            if !key.is_empty() && k == key && i != id {
                *k = String::new();
                changed.push(i.clone());
            }
        }
        if let Some((_, k)) = binds.iter_mut().find(|(i, _)| i == id) {
            *k = key.to_string();
            changed.push(id.to_string());
        }
    });
    for i in changed {
        let value = key_of(game, &i);
        game.native_set_setting(ID, &bind_file(&i), &value);
    }
}

/// Binds every action to its default key.
fn reset_binds(game: &Game<'_>) {
    for (id, _, default) in BINDS {
        set_bind(game, id, default);
    }
}

/// The Keys tab's capture: the next key pressed on the screen is bound to the action waiting for
/// one; Backspace and Delete clear it. Esc is the screen's own and closes it, which ends the wait.
fn capture_key(game: &Game<'_>, ui: &mut menus::Ui<'_>) {
    use winit::keyboard::KeyCode;
    const BINDABLE: &[KeyCode] = &[
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG,
    KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN,
    KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU,
    KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ, KeyCode::Digit1,
    KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7,
    KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0, KeyCode::Enter, KeyCode::Escape, KeyCode::Backspace,
    KeyCode::Tab, KeyCode::Space, KeyCode::Minus, KeyCode::Equal, KeyCode::BracketLeft, KeyCode::BracketRight,
    KeyCode::Backslash, KeyCode::Semicolon, KeyCode::Quote, KeyCode::Backquote, KeyCode::Comma,
    KeyCode::Period, KeyCode::Slash, KeyCode::CapsLock, KeyCode::IntlBackslash, KeyCode::IntlRo,
    KeyCode::IntlYen, KeyCode::Lang1, KeyCode::Lang2, KeyCode::Lang3, KeyCode::Lang4, KeyCode::Lang5,
    KeyCode::Help, KeyCode::Select, KeyCode::Again, KeyCode::Undo, KeyCode::Cut, KeyCode::Copy,
    KeyCode::Paste, KeyCode::Find, KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5,
    KeyCode::F6, KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
    KeyCode::F13, KeyCode::F14, KeyCode::F15, KeyCode::F16, KeyCode::F17, KeyCode::F18, KeyCode::F19,
    KeyCode::F20, KeyCode::F21, KeyCode::F22, KeyCode::F23, KeyCode::F24, KeyCode::PrintScreen,
    KeyCode::ScrollLock, KeyCode::Pause, KeyCode::Insert, KeyCode::Home, KeyCode::PageUp, KeyCode::Delete,
    KeyCode::End, KeyCode::PageDown, KeyCode::ArrowRight, KeyCode::ArrowLeft, KeyCode::ArrowDown,
    KeyCode::ArrowUp, KeyCode::NumLock, KeyCode::NumpadDivide, KeyCode::NumpadMultiply,
    KeyCode::NumpadSubtract, KeyCode::NumpadAdd, KeyCode::NumpadEnter, KeyCode::Numpad1, KeyCode::Numpad2,
    KeyCode::Numpad3, KeyCode::Numpad4, KeyCode::Numpad5, KeyCode::Numpad6, KeyCode::Numpad7,
    KeyCode::Numpad8, KeyCode::Numpad9, KeyCode::Numpad0, KeyCode::NumpadDecimal, KeyCode::NumpadEqual,
    KeyCode::NumpadComma, KeyCode::NumpadBackspace, KeyCode::NumpadHash, KeyCode::NumpadMemoryAdd,
    KeyCode::NumpadClear, KeyCode::NumpadClearEntry, KeyCode::ContextMenu, KeyCode::ControlLeft,
    KeyCode::ShiftLeft, KeyCode::AltLeft, KeyCode::SuperLeft, KeyCode::ControlRight, KeyCode::ShiftRight,
    KeyCode::AltRight, KeyCode::SuperRight,
    ];
    let Some(id) = with(|s| s.capture.clone()) else { return };
    let Some(code) = BINDABLE.iter().copied().find(|&code| ui.key(code)) else { return };
    let name = if matches!(code, KeyCode::Backspace | KeyCode::Delete) { Some("") } else { horizon_client::keybinds::key_name(code) };
    let Some(name) = name else { return };
    set_bind(game, &id, name);
    let label = BINDS.iter().find(|(i, ..)| *i == id).map_or("", |(_, l, _)| *l);
    with(|s| {
        s.capture = None;
        s.ui_message = format!("{label}: {}", key_label(name));
    });
}

pub(super) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(ID) || !game.native_playing() {
        return;
    }
    let hit = with(|s| {
        // Back in the game: a wait for a key on the Keys tab is over.
        s.capture = None;
        ensure_binds(game, s);
        s.binds.as_ref().and_then(|b| b.iter().find(|(_, k)| !k.is_empty() && k == key)).map(|(i, _)| i.clone())
    });
    let hit = hit.as_deref();
    if hit == Some("move_mode") {
        toggle_move_mode(game);
        return;
    }
    if hit == Some("open_menu") {
        // As Litematica's main menu: always from its first page.
        // Promatica's own screen; the mod's page in Options > Mods when it can't open.
        if !menus::open_screen(game) {
            with(|s| s.nav.reset());
            if !menus::open_page(game, ID) {
                say(game, "The menu is in Options > Mods > Promatica");
            }
        }
        return;
    }
    // In move mode its keys come first (Page up/down and Home move instead of changing layers).
    if with(|s| s.moving) && move_key(game, key) {
        return;
    }
    let Some(action) = hit else { return };
    if action == "toggle_materials" {
        // The HUD list only: don't fill the chat each press.
        with(|s| {
            s.settings.materials_hud = !s.settings.materials_hud;
            save_settings(game, &s.settings);
        });
        return;
    }
    menu_action(game, action);
}

// ---------------------------------------------------------------- move mode

fn toggle_move_mode(game: &mut Game<'_>) {
    if with(|s| !s.moving && s.sel().is_some_and(|x| x.locked)) {
        say(game, "Move mode: the placement is locked (/hm unlock)");
        return;
    }
    let on = with(|s| {
        s.placement()?;
        s.moving = !s.moving;
        s.wheel = 0.0;
        Some(s.moving)
    });
    match on {
        None => say(game, "Move mode: no schematic loaded"),
        Some(true) => say(
            game,
            "Move mode ON: wheel = along your view, arrows = horizontal, Page up/down = up/down, R = rotate, Home = to you, Shift = 5 blocks. N or Enter: done",
        ),
        Some(false) => {
            save_placements(game);
            say(game, "Move mode OFF")
        }
    }
}

/// Blocks per step: 5 with Shift held.
fn move_step(game: &Game<'_>) -> i32 {
    use winit::keyboard::KeyCode;
    if game.keys.contains(&KeyCode::ShiftLeft) || game.keys.contains(&KeyCode::ShiftRight) { 5 } else { 1 }
}

/// The unit axis closest to `dir`.
fn dominant(dir: [f64; 3]) -> [i32; 3] {
    let i = (0..3).max_by(|&a, &b| dir[a].abs().total_cmp(&dir[b].abs())).unwrap_or(0);
    let mut out = [0; 3];
    out[i] = if dir[i] < 0.0 { -1 } else { 1 };
    out
}

/// Moves the placement by `d` without a chat line (the HUD shows the origin). False when it is
/// locked.
fn shift(d: [i32; 3]) -> bool {
    with(|s| {
        if s.sel().is_some_and(|x| x.locked) {
            return false;
        }
        if let Some(p) = s.placement_mut() {
            p.nudge(d);
            s.scan = Scan::default();
            s.mesh_key = None;
        }
        true
    })
}

/// A key in move mode: whether it was one of move mode's.
fn move_key(game: &mut Game<'_>, key: &str) -> bool {
    let k = move_step(game);
    let view = super::view_vector(game.yaw, 0.0);
    let fwd = dominant([view[0], 0.0, view[2]]);
    let right = [-fwd[2], 0, fwd[0]];
    let d = match key {
        "key.keyboard.up" => fwd,
        "key.keyboard.down" => fwd.map(|v| -v),
        "key.keyboard.right" => right,
        "key.keyboard.left" => right.map(|v| -v),
        "key.keyboard.page.up" => [0, 1, 0],
        "key.keyboard.page.down" => [0, -1, 0],
        "key.keyboard.r" => {
            let r = run(game, &["rotate", "cw"]);
            if let Err(e) = r {
                say(game, &format!("Error: {e}"));
            }
            return true;
        }
        "key.keyboard.home" => {
            let r = run(game, &["origin", "here"]);
            if let Err(e) = r {
                say(game, &format!("Error: {e}"));
            }
            return true;
        }
        "key.keyboard.enter" | "key.keyboard.keypad.enter" => {
            toggle_move_mode(game);
            return true;
        }
        _ => return false,
    };
    if !shift(d.map(|v| v * k)) {
        say(game, "The placement is locked (/hm unlock)");
    }
    true
}

/// The wheel in move mode: the placement moves along the axis you look along, away from you
/// when scrolled up. Claimed, so the hotbar stays put.
pub(super) fn wheel(game: &mut Game<'_>, steps: f32, claimed: &mut bool) {
    if !game.native_on(ID) || !game.native_playing() {
        return;
    }
    if !with(|s| s.moving && s.placement().is_some()) {
        tool_wheel(game, steps, claimed);
        return;
    }
    *claimed = true;
    let n = with(|s| {
        s.wheel += steps;
        let n = s.wheel.trunc();
        s.wheel -= n;
        n as i32
    });
    if n == 0 {
        return;
    }
    let axis = dominant(super::view_vector(game.yaw, game.pitch));
    let k = n * move_step(game);
    if !shift(axis.map(|v| v * k)) {
        say(game, "The placement is locked (/hm unlock)");
    }
}

fn report(game: &mut Game<'_>, r: Result<Vec<String>, String>) {
    match r {
        Ok(lines) => {
            for l in lines {
                say(game, &l);
            }
        }
        Err(e) => say(game, &format!("Error: {e}")),
    }
}

// ---------------------------------------------------------------- per tick

pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() || !game.native_on(ID) {
        return;
    }
    if game.player.is_none() {
        return;
    }
    with(|s| load_settings(game, s));
    restore_step(game);
    load_step(game);
    with(State::sync_rev);
    scan_step(game);
    save_step(game);
    rebuild_mesh(game);
    marks_step(game);
    printer_step(game);
    easy_place_step(game);
    completions_step(game);
}

/// Leaving a world, or the mod switched off: the overlay is dropped. Otherwise the render side
/// gets the current draw options (they need no new geometry).
pub(super) fn frame(game: &mut Game<'_>) {
    let active = game.player.is_some() && game.native_on(ID);
    with(|s| {
        if !active {
            if !s.mesh.built.is_empty() || s.mesh_key.is_some() {
                s.mesh = Mesh::default();
                s.mesh_key = None;
                bridge::clear();
            }
            return;
        }
        let any = s.slots.iter().any(|x| x.shown) || s.area.iter().any(Option::is_some) || s.save.is_some();
        bridge::options(s.settings.through, s.settings.render && any);
    });
}

/// The verifier, over and over, for the selected placement alone (shown or not): its progress
/// in the HUD and its material list. Another placement selected starts it over.
fn scan_step(game: &Game<'_>) {
    with(|s| {
        let Some(slot) = s.sel() else { return };
        let (sel, key) = (s.selected, (slot.id, slot.placement.revision));
        let (lo, hi) = slot.placement.bounds();
        let size = [0, 1, 2].map(|i| (hi[i] - lo[i] + 1) as u64);
        let volume = size[0] * size[1] * size[2];
        if s.scan.key != key {
            s.scan = Scan { key, ..Scan::default() };
            // Moved or turned, the last result shows until the new one is in; another
            // placement's doesn't.
            if s.tally_of != key.0 {
                s.tally = None;
                s.missing.clear();
                s.unloaded = 0;
            }
        }
        let end = (s.scan.index + SCAN_PER_TICK).min(volume);
        let mut i = s.scan.index;
        while i < end {
            let x = (i % size[0]) as i32;
            let z = ((i / size[0]) % size[2]) as i32;
            let y = (i / (size[0] * size[2])) as i32;
            i += 1;
            let w = [lo[0] + x, lo[1] + y, lo[2] + z];
            if !loaded(game, w) {
                if s.slots[sel].placement.wanted(w).is_some_and(|b| !b.is_air()) {
                    s.scan.unloaded += 1;
                }
                continue;
            }
            let Some((status, at, _)) = quick_status_in(game, s, sel, w) else { continue };
            let wanted = s.state_of(at).clone();
            s.scan.tally.add(status);
            if matches!(status, Status::Missing | Status::WrongBlock) {
                let per = u64::from(materials::items_for_state(&wanted));
                if per > 0
                    && let Some(item) = item_for(game, s, &wanted)
                {
                    *s.scan.missing.entry(item).or_insert(0) += per;
                }
            }
        }
        s.scan.index = i;
        if i >= volume {
            let done = std::mem::replace(&mut s.scan, Scan { key, ..Scan::default() });
            s.tally_of = key.0;
            s.tally = Some(done.tally);
            s.missing = done.missing;
            s.unloaded = done.unloaded;
        }
    });
}

fn argb_to_vertex(argb: u32, shade: f32) -> u32 {
    let [a, r, g, b] = argb.to_be_bytes();
    let f = |c: u8| (f32::from(c) * shade).round().clamp(0.0, 255.0) as u32;
    f(r) | f(g) << 8 | f(b) << 16 | u32::from(a) << 24
}

/// A box's six faces as quads (positions relative to the anchor), on the atlas's white texel.
/// The 12 edges of the box `lo..hi` as thin boxes `t` thick: along each axis, at the four
/// combinations of the other two.
fn push_frame(out: &mut Vec<Vertex>, white: [f32; 2], lo: [f32; 3], hi: [f32; 3], t: f32, argb: u32) {
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for cu in [lo[u], hi[u]] {
            for cv in [lo[v], hi[v]] {
                let mut min = [0.0; 3];
                let mut max = [0.0; 3];
                min[axis] = lo[axis] - t;
                max[axis] = hi[axis] + t;
                min[u] = cu - t;
                max[u] = cu + t;
                min[v] = cv - t;
                max[v] = cv + t;
                push_box(out, white, min, max, argb);
            }
        }
    }
}

fn push_box(out: &mut Vec<Vertex>, white: [f32; 2], min: [f32; 3], max: [f32; 3], argb: u32) {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    let faces: [([[f32; 3]; 4], f32); 6] = [
        ([[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]], 1.0),
        ([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], 0.5),
        ([[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]], 0.8),
        ([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], 0.8),
        ([[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]], 0.6),
        ([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]], 0.6),
    ];
    for (corners, shade) in faces {
        let color = argb_to_vertex(argb, shade);
        for pos in corners {
            out.push(Vertex {
                pos,
                uv: white,
                layer: 0,
                color,
                light: FULL_BRIGHT,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }
}

/// A block face's colour with vanilla's directional shade (`BlockModelShaper`: up 1, down 0.5,
/// north/south 0.8, east/west 0.6), from its normal.
fn shade(color: u32, normal: [i8; 4]) -> u32 {
    let [nx, ny, nz, _] = normal.map(i32::from);
    let shade = if ny > 64 {
        255
    } else if ny < -64 {
        127
    } else if nz.abs() >= nx.abs() {
        204
    } else {
        153
    };
    let [r, g, b, a] = color.to_le_bytes();
    let f = |c: u8| (u32::from(c) * shade / 255) as u8;
    u32::from_le_bytes([f(r), f(g), f(b), a])
}

/// The world's section `k` (chunk x, section y, chunk z), shared: the world copies a section
/// before writing to it while someone else holds it, so a different pointer later means it
/// changed (or its chunk was loaded again).
fn world_section(game: &Game<'_>, k: [i32; 3]) -> Option<Arc<horizon_client::chunk::Section>> {
    let chunk = game.world.chunks.get(&(k[0], k[2]))?;
    let i = chunk.section_index(k[1] * 16)?;
    chunk.sections.get(i).cloned()
}

fn same_section(a: &Option<Arc<horizon_client::chunk::Section>>, b: &Option<Arc<horizon_client::chunk::Section>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Time the overlay may take per tick to build sections.
const MESH_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);

/// Keeps the overlay up to date, one 16x16x16 section at a time: a section is built again only
/// when its world section changed (a block set, a chunk loaded) or what it shows did (the
/// placement, layers, display settings), nearest first, and only as many per tick as fit in
/// `MESH_BUDGET`. The render side keeps each section's buffer, so a change uploads one section.
fn rebuild_mesh(game: &Game<'_>) {
    let Some(me) = feet(game) else { return };
    let start = std::time::Instant::now();
    if bridge::take_lost() {
        with(|s| s.mesh_key = None);
    }
    let Some((key, anchor, boxes, radius, limit)) = with(|s| {
        let st = &s.settings;
        let key = (s.rev, s.layer, st.mode, st.show_extra, st.show_wrong, st.strict, st.power, st.alpha.to_bits(), st.hide_fluids);
        let boxes: Vec<_> = s.slots.iter().filter(|x| x.shown).map(|x| x.placement.bounds()).collect();
        // Positions are relative to the placements' corner, else (only an area) the area's.
        let anchor = s
            .shown_bounds()
            .or_else(|| s.area_box())
            .or_else(|| s.area.iter().flatten().next().map(|&c| (c, c)))
            .or_else(|| s.save.as_ref().map(|j| (j.lo, j.hi)))?
            .0;
        let limit = if st.max_faces == 0 { usize::MAX } else { st.max_faces };
        Some((key, anchor, boxes, render_radius(game, st), limit))
    }) else {
        if with(|s| std::mem::take(&mut s.mesh).built.len()) > 0 || with(|s| s.mesh_key.take()).is_some() {
            bridge::clear();
        }
        return;
    };
    // What every section shows changed: start over from the nearest.
    if with(|s| s.mesh_key != Some(key) || s.mesh.anchor != anchor) {
        with(|s| {
            s.mesh_key = Some(key);
            s.mesh = Mesh { anchor, ..Mesh::default() };
        });
        bridge::clear();
        bridge::set_anchor(anchor);
    }
    area_step(game);
    save_frames_step(game);
    placement_frames_step(game);
    // The sections near the player that a shown placement reaches into. A large radius is gone
    // through whole only every `FAR_EVERY` ticks; in between only the near part (changes the
    // player makes are there).
    let full = with(|s| {
        s.mesh_tick += 1;
        s.mesh_tick % FAR_EVERY == 1
    }) || radius <= NEAR;
    let reach = if full { radius } else { NEAR };
    let mut wanted: std::collections::HashSet<[i32; 3]> = std::collections::HashSet::new();
    for (lo, hi) in boxes {
        let a = [0, 1, 2].map(|i| lo[i].max(me[i] - reach) >> 4);
        let b = [0, 1, 2].map(|i| hi[i].min(me[i] + reach) >> 4);
        for sy in a[1]..=b[1] {
            for sz in a[2]..=b[2] {
                for sx in a[0]..=b[0] {
                    wanted.insert([sx, sy, sz]);
                }
            }
        }
    }
    // Out of range: dropped (known only when the whole radius was gone through).
    let gone: Vec<[i32; 3]> =
        if full { with(|s| s.mesh.built.keys().filter(|k| !wanted.contains(*k)).copied().collect()) } else { Vec::new() };
    for k in gone {
        with(|s| {
            if let Some((_, n)) = s.mesh.built.remove(&k) {
                s.mesh.quads -= n;
            }
        });
        bridge::remove_section(k);
    }
    // New or changed: built again, nearest first.
    let mut todo = Vec::new();
    with(|s| {
        for &k in &wanted {
            let now = world_section(game, k);
            let stale = s.mesh.built.get(&k).is_none_or(|(snap, _)| !same_section(snap, &now));
            if stale {
                let d = [0, 1, 2].map(|i| i64::from(k[i] * 16 + 8 - me[i]));
                todo.push((d[0] * d[0] + d[1] * d[1] + d[2] * d[2], k, now));
            }
        }
    });
    todo.sort_by_key(|t| (t.0, t.1));
    // The near (or, on a full tick, all) stale sections first, then the far queue.
    let mut queue = if full { std::collections::VecDeque::new() } else { with(|s| std::mem::take(&mut s.mesh.queue)) };
    let mut todo = todo.into_iter().map(|(_, k, snap)| (k, Some(snap)));
    let mut out_of_time = false;
    let mut full_note = false;
    loop {
        let (k, snap) = match todo.next() {
            Some(t) => t,
            None if out_of_time || full_note => break,
            None => match queue.pop_front() {
                Some(k) => (k, None),
                None => break,
            },
        };
        if out_of_time || full_note {
            // Kept for the next ticks.
            if full {
                queue.push_back(k);
            }
            continue;
        }
        let snap = match snap {
            Some(snap) => snap,
            None => {
                let now = world_section(game, k);
                if with(|s| s.mesh.built.get(&k).is_some_and(|(snap, _)| same_section(snap, &now))) {
                    continue;
                }
                now
            }
        };
        if with(|s| s.mesh.quads >= limit && !s.mesh.built.contains_key(&k)) {
            with(|s| s.printer_note = format!("overlay full ({}): raise the face limit or lower the render radius", faces_text(limit)));
            full_note = true;
            if full {
                queue.push_back(k);
            }
            continue;
        }
        let quads = build_section(game, k);
        let n = quads.len() / 4;
        with(|s| {
            let old = s.mesh.built.insert(k, (snap, n)).map_or(0, |(_, n)| n);
            s.mesh.quads = s.mesh.quads + n - old;
        });
        bridge::put_section(k, quads);
        out_of_time = start.elapsed() > MESH_BUDGET;
    }
    with(|s| {
        s.mesh.todo = queue.len();
        s.mesh.queue = queue;
    });
}

/// What the highlights show, in words.
fn highlight_text(s: &State) -> String {
    let what = match (s.settings.highlight_missing, s.settings.highlight_wrong) {
        (false, false) => return "Highlight off".to_string(),
        (true, false) => "missing",
        (false, true) => "wrong",
        (true, true) => "missing and wrong",
    };
    match &s.highlight_item {
        Some(item) => format!("Highlight: {what} blocks, only {}", short(item)),
        None => format!("Highlight: {what} blocks"),
    }
}

/// Keeps the highlights up to date for the shown placements, however far they reach (the render
/// radius and the face limit don't count here). Like `rebuild_mesh`, a section is gone through
/// again only when its world section or what it shows changed, nearest first.
fn marks_step(game: &Game<'_>) {
    let Some(me) = feet(game) else { return };
    let start = std::time::Instant::now();
    let Some((changed, on, boxes)) = with(|s| {
        s.mesh_key?;
        let key = (s.settings.highlight_missing, s.settings.highlight_wrong, s.highlight_item.clone());
        let changed = s.mesh.marks_key.as_ref() != Some(&key);
        let on = key.0 || key.1;
        if changed {
            s.mesh.marks_key = Some(key);
            s.mesh.marks.clear();
            s.mesh.marks_count = 0;
            s.mesh.marks_queue.clear();
            s.mesh.marks_full = false;
            bridge::clear_marks();
        }
        let boxes: Vec<_> = s.slots.iter().filter(|x| x.shown).map(|x| x.placement.bounds()).collect();
        Some((changed, on, boxes))
    }) else {
        return;
    };
    if !on {
        return;
    }
    let full = changed || with(|s| s.mesh_tick % FAR_EVERY == 1);
    let reach = if full { i32::MAX / 2 } else { NEAR };
    let mut wanted: std::collections::HashSet<[i32; 3]> = std::collections::HashSet::new();
    for (lo, hi) in boxes {
        let a = [0, 1, 2].map(|i| lo[i].max(me[i].saturating_sub(reach)) >> 4);
        let b = [0, 1, 2].map(|i| hi[i].min(me[i].saturating_add(reach)) >> 4);
        for sy in a[1]..=b[1] {
            for sz in a[2]..=b[2] {
                for sx in a[0]..=b[0] {
                    wanted.insert([sx, sy, sz]);
                }
            }
        }
    }
    let dist = |k: [i32; 3]| {
        let d = [0, 1, 2].map(|i| i64::from(k[i]) * 16 + 8 - i64::from(me[i]));
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    };
    let mut todo: Vec<[i32; 3]> = with(|s| {
        if full {
            // Gone from the placements: dropped.
            let gone: Vec<_> = s.mesh.marks.keys().filter(|k| !wanted.contains(*k)).copied().collect();
            for k in gone {
                if let Some((_, n)) = s.mesh.marks.remove(&k) {
                    s.mesh.marks_count -= n;
                }
                bridge::put_marks(k, Vec::new());
            }
            s.mesh.marks_full = false;
            s.mesh.marks_queue.clear();
        }
        wanted
            .iter()
            .filter(|&&k| s.mesh.marks.get(&k).is_none_or(|(snap, _)| !same_section(snap, &world_section(game, k))))
            .copied()
            .collect()
    });
    todo.sort_by_key(|&k| (dist(k), k));
    let mut queue: std::collections::VecDeque<[i32; 3]> = todo.into();
    queue.extend(with(|s| std::mem::take(&mut s.mesh.marks_queue)));
    while let Some(k) = queue.pop_front() {
        let snap = world_section(game, k);
        if with(|s| s.mesh.marks.get(&k).is_some_and(|(old, _)| same_section(old, &snap))) {
            continue;
        }
        let (quads, n) = build_marks(game, k);
        // Over the limit: the farthest make room for nearer ones; if none is farther, this is as
        // far as it goes, until the player moves.
        let fits = with(|s| {
            let old = s.mesh.marks.get(&k).map_or(0, |(_, n)| *n);
            while s.mesh.marks_count - old + n > MAX_MARKS {
                let far = s.mesh.marks.iter().filter(|(q, (_, m))| **q != k && *m > 0).map(|(q, _)| *q).max_by_key(|&q| dist(q));
                let Some(far) = far.filter(|&q| dist(q) > dist(k)) else {
                    s.mesh.marks_full = true;
                    return false;
                };
                if let Some((_, m)) = s.mesh.marks.remove(&far) {
                    s.mesh.marks_count -= m;
                }
                s.mesh.marks_full = true;
                bridge::put_marks(far, Vec::new());
            }
            s.mesh.marks_count = s.mesh.marks_count - old + n;
            s.mesh.marks.insert(k, (snap, n));
            true
        });
        if !fits {
            queue.clear();
            break;
        }
        bridge::put_marks(k, quads);
        if start.elapsed() > MARKS_BUDGET {
            break;
        }
    }
    with(|s| s.mesh.marks_queue = queue);
}

/// The highlights of section `k`: a box over each block to mark, seen through everything, and how
/// many there are. Missing and wrong go by the item wanted there, as the material list counts.
fn build_marks(game: &Game<'_>, k: [i32; 3]) -> (Vec<Vertex>, usize) {
    let mut quads = Vec::new();
    let white = horizon_client::gfx::lightning::white_uv(&game.gui.atlas.font.white());
    let mut count = 0;
    with(|s| {
        let Some((lo, hi)) = s.shown_bounds() else { return };
        let anchor = s.mesh.anchor;
        let (missing, wrong) = (s.settings.highlight_missing, s.settings.highlight_wrong);
        let hide_fluids = s.settings.hide_fluids;
        let only = s.highlight_item.clone();
        let layer = s.layer;
        let a = [0, 1, 2].map(|i| (k[i] * 16).max(lo[i]));
        let b = [0, 1, 2].map(|i| (k[i] * 16 + 15).min(hi[i]));
        for y in a[1]..=b[1] {
            if !layer.contains(y) {
                continue;
            }
            for z in a[2]..=b[2] {
                for x in a[0]..=b[0] {
                    let w = [x, y, z];
                    if !loaded(game, w) {
                        continue;
                    }
                    let Some((status, at, res)) = quick_status(game, s, w) else { continue };
                    let argb = match status {
                        Status::Missing if missing => 0x8033_B3E6u32,
                        Status::WrongBlock if wrong => 0x80FF_3333,
                        Status::WrongState if wrong => 0x80FF_9010,
                        _ => continue,
                    };
                    if hide_fluids && res.fluid != 0 {
                        continue;
                    }
                    if let Some(only) = &only {
                        let wanted = s.state_of(at).clone();
                        if item_for(game, s, &wanted).as_ref() != Some(only) {
                            continue;
                        }
                    }
                    let lo = [0, 1, 2].map(|i| (w[i] - anchor[i]) as f32 - 0.01);
                    let start = quads.len();
                    push_box(&mut quads, white, lo, lo.map(|v| v + 1.02), argb);
                    // Outward normals in `push_box`'s order: only the near faces are drawn, so a
                    // block's far side doesn't darken it.
                    const OUT: [[i8; 4]; 6] =
                        [[0, 127, 0, 127], [0, -127, 0, 127], [0, 0, -127, 127], [0, 0, 127, 127], [-127, 0, 0, 127], [127, 0, 0, 127]];
                    for (face, q) in quads[start..].chunks_exact_mut(4).enumerate() {
                        for v in q {
                            v.normal = OUT[face];
                        }
                    }
                    count += 1;
                }
            }
        }
    });
    (quads, count)
}

/// Hands the area selection's frame to the render side when it changed: the box's edges, corner 1
/// in red and corner 2 in blue (as Litematica marks them).
fn area_step(game: &Game<'_>) {
    let tool = tool_held(game);
    let Some((area, anchor, grab)) = with(|s| {
        let now = (s.area, s.mesh.anchor, tool.then_some(s.grab));
        (s.mesh.area != Some(now)).then(|| {
            s.mesh.area = Some(now);
            now
        })
    }) else {
        return;
    };
    let mut quads = Vec::new();
    let white = horizon_client::gfx::lightning::white_uv(&game.gui.atlas.font.white());
    let rel = |p: [i32; 3]| [0, 1, 2].map(|i| (p[i] - anchor[i]) as f32);
    // The corner the selection tool has picked: stronger and a little larger.
    for (i, (corner, argb)) in [(area[0], 0x80FF_3030u32), (area[1], 0x8030_60FF)].into_iter().enumerate() {
        if let Some(c) = corner {
            let r = rel(c);
            let (argb, out) = if grab == Some(Grab::Corner(i)) { (argb | 0xD000_0000, 0.04) } else { (argb, 0.01) };
            push_box(&mut quads, white, r.map(|v| v - out), r.map(|v| v + 1.0 + out), argb);
        }
    }
    // The whole box picked: yellow edges.
    let edge = if grab == Some(Grab::Box) { 0xE0FF_FF55 } else { 0xE0FF_FFFF };
    if let [Some(a), Some(b)] = area {
        let lo = rel([0, 1, 2].map(|i| a[i].min(b[i])));
        let hi = rel([0, 1, 2].map(|i| a[i].max(b[i]) + 1));
        push_frame(&mut quads, white, lo, hi, 0.03, edge);
    }
    if quads.is_empty() {
        bridge::remove_section(AREA_SECTION);
    } else {
        bridge::put_section(AREA_SECTION, quads);
    }
}

/// Hands the frames around the placements to the render side when they changed: the selected
/// one bright (cyan, or orange when locked) and thicker, the other shown ones faint.
fn placement_frames_step(game: &Game<'_>) {
    let Some((frames, anchor)) = with(|s| {
        let frames: Vec<_> = if s.settings.placement_boxes {
            s.slots
                .iter()
                .enumerate()
                .filter(|(i, x)| x.shown || *i == s.selected)
                .map(|(i, x)| {
                    let (lo, hi) = x.placement.bounds();
                    let (argb, thick) = match (i == s.selected, x.locked) {
                        (true, false) => (0xE055_FFFF, 4),
                        (true, true) => (0xE0FF_AA00, 4),
                        (false, _) => (0x70C0_C0C0, 2),
                    };
                    (lo, hi, argb, thick)
                })
                .collect()
        } else {
            Vec::new()
        };
        let now = (frames, s.mesh.anchor);
        (s.mesh.frames.as_ref() != Some(&now)).then(|| {
            s.mesh.frames = Some(now.clone());
            now
        })
    }) else {
        return;
    };
    let white = horizon_client::gfx::lightning::white_uv(&game.gui.atlas.font.white());
    let rel = |p: [i32; 3]| [0, 1, 2].map(|i| (p[i] - anchor[i]) as f32);
    let mut quads = Vec::new();
    for (lo, hi, argb, thick) in frames {
        push_frame(&mut quads, white, rel(lo), rel(hi.map(|v| v + 1)), thick as f32 / 100.0, argb);
    }
    if quads.is_empty() {
        bridge::remove_section(PLACEMENT_SECTION);
    } else {
        bridge::put_section(PLACEMENT_SECTION, quads);
    }
}

/// The overlay's quads for section `k`: ghosts of missing blocks (the faces between two ghost
/// cubes left out), boxes for the rest, each quad with its outward normal for the render side.
fn build_section(game: &Game<'_>, k: [i32; 3]) -> Vec<Vertex> {
    let mut quads = Vec::new();
    let white = horizon_client::gfx::lightning::white_uv(&game.gui.atlas.font.white());
    with(|s| {
        let Some((lo, hi)) = s.shown_bounds() else { return };
        let anchor = s.mesh.anchor;
        let ghost_alpha = (s.settings.alpha.clamp(0.05, 1.0) * 255.0) as u32;
        let (mode, show_extra, show_wrong) = (s.settings.mode, s.settings.show_extra, s.settings.show_wrong);
        let hide_fluids = s.settings.hide_fluids;
        let layer = s.layer;
        let a = [0, 1, 2].map(|i| (k[i] * 16).max(lo[i]));
        let b = [0, 1, 2].map(|i| (k[i] * 16 + 15).min(hi[i]));
        // What each cell of the section and the layer of cells around it shows, as far as it
        // hides a neighbour's face: `NOTHING`, `CUBE` (a ghost full cube) or `WATER`/`LAVA` (a
        // ghost fluid). Filled in while building, the border on demand; `UNKNOWN` until then.
        const UNKNOWN: u8 = 255;
        const NOTHING: u8 = 0;
        const CUBE: u8 = 1;
        let base = [0, 1, 2].map(|i| k[i] * 16 - 1);
        let cell = |w: [i32; 3]| -> Option<usize> {
            let l = [0, 1, 2].map(|i| w[i] - base[i]);
            l.iter().all(|v| (0..18).contains(v)).then(|| ((l[1] * 18 + l[2]) * 18 + l[0]) as usize)
        };
        let mut shows = vec![UNKNOWN; 18 * 18 * 18];
        let class = |status: Status, res: &Resolved| -> u8 {
            if mode != Mode::Ghost || status != Status::Missing || res.id.is_none() {
                NOTHING
            } else if res.cube {
                CUBE
            } else if res.fluid != 0 && !hide_fluids {
                1 + res.fluid
            } else {
                NOTHING
            }
        };
        // Per quad: the block it belongs to, and what that block shows.
        let mut blocks: Vec<([i32; 3], u8)> = Vec::new();
        for y in a[1]..=b[1] {
            if !layer.contains(y) {
                continue;
            }
            for z in a[2]..=b[2] {
                for x in a[0]..=b[0] {
                    let w = [x, y, z];
                    if !loaded(game, w) {
                        continue;
                    }
                    let Some((status, _, res)) = quick_status(game, s, w) else {
                        if let Some(c) = cell(w) {
                            shows[c] = NOTHING;
                        }
                        continue;
                    };
                    let shown = class(status, &res);
                    if let Some(c) = cell(w) {
                        shows[c] = shown;
                    }
                    if hide_fluids && res.fluid != 0 {
                        continue;
                    }
                    let rel = [0, 1, 2].map(|i| (w[i] - anchor[i]) as f32);
                    let inflated = |e: f32| ([rel[0] - e, rel[1] - e, rel[2] - e], [rel[0] + 1.0 + e, rel[1] + 1.0 + e, rel[2] + 1.0 + e]);
                    let mut cube = false;
                    match status {
                        Status::Correct => {}
                        Status::Missing => {
                            let start = quads.len();
                            if mode == Mode::Ghost
                                && let Some(id) = res.id
                            {
                                let at = glam::Mat4::from_translation(glam::Vec3::from_array(rel));
                                // With `diffuse` each face comes with its own normal (a repeater's
                                // top lies below the block's middle and still faces up); the
                                // directional shade the game would bake in is added here.
                                game.block_model_vertices(id, 0, at, FULL_BRIGHT, true, &mut quads);
                                for v in &mut quads[start..] {
                                    v.color = shade(v.color, v.normal);
                                }
                                // Chests, shulker boxes, banners, heads, bells...: the block
                                // entity's model (a double chest's half by its `type`).
                                game.special_block_model(id, at, 0xFF, &mut quads);
                                // The ghost's opacity in the colour's alpha.
                                for v in &mut quads[start..] {
                                    v.color = (v.color & 0x00FF_FFFF) | ghost_alpha << 24;
                                }
                                cube = shown != NOTHING;
                            }
                            if quads.len() == start {
                                let (lo, hi) = inflated(-0.2);
                                push_box(&mut quads, white, lo, hi, Status::Missing.overlay_argb() | 0x6000_0000);
                            }
                        }
                        Status::Extra if show_extra => {
                            let (lo, hi) = inflated(0.004);
                            push_box(&mut quads, white, lo, hi, Status::Extra.overlay_argb());
                        }
                        Status::WrongBlock | Status::WrongState if show_wrong => {
                            let (lo, hi) = inflated(0.004);
                            push_box(&mut quads, white, lo, hi, status.overlay_argb());
                        }
                        _ => {}
                    }
                    while blocks.len() < quads.len() / 4 {
                        blocks.push((w, if cube { shown } else { NOTHING }));
                    }
                }
            }
        }
        quads.truncate(blocks.len() * 4);
        // What the cell `w` shows (the border around the section is looked up once, here).
        let mut shows_at = |w: [i32; 3]| -> u8 {
            let Some(c) = cell(w) else { return NOTHING };
            if shows[c] == UNKNOWN {
                shows[c] = if layer.contains(w[1]) && loaded(game, w) {
                    quick_status(game, s, w).map_or(NOTHING, |(st, _, res)| class(st, &res))
                } else {
                    NOTHING
                };
            }
            shows[c]
        };
        let mut kept = Vec::with_capacity(quads.len());
        for (q, &(w, own)) in quads.chunks_exact(4).zip(&blocks) {
            let block = glam::Vec3::new((w[0] - anchor[0]) as f32 + 0.5, (w[1] - anchor[1]) as f32 + 0.5, (w[2] - anchor[2]) as f32 + 0.5);
            let p = [0, 1, 2, 3].map(|i| glam::Vec3::from_array(q[i].pos));
            let centre = (p[0] + p[1] + p[2] + p[3]) * 0.25;
            let out = centre - block;
            let given = q[0].normal;
            let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            // The model's own normal where it gives one (two-sided faces, w = -127, never hide);
            // for the overlay's boxes, away from the block's centre (their faces are its sides).
            // A face through the centre (a plant's cross) gets none and is always drawn.
            let normal = if given[3] == -127 {
                glam::Vec3::ZERO
            } else if given[..3] != [0, 0, 0] {
                glam::Vec3::new(f32::from(given[0]), f32::from(given[1]), f32::from(given[2])).normalize_or_zero()
            } else if out.length_squared() < 1e-4 || normal.dot(out).abs() < 1e-3 {
                glam::Vec3::ZERO
            } else if normal.dot(out) < 0.0 {
                -normal
            } else {
                normal
            };
            // A ghost cube's face on its boundary, against another ghost cube: hidden; a ghost
            // fluid's against a cube or the same fluid too.
            if own != NOTHING && normal != glam::Vec3::ZERO {
                let axis = (0..3).find(|&i| normal[i].abs() > 0.99);
                if let Some(i) = axis
                    && (out[i].abs() - 0.5).abs() < 1e-3
                {
                    let mut n = w;
                    n[i] += normal[i].signum() as i32;
                    let next = shows_at(n);
                    if next == CUBE || (own != CUBE && next == own) {
                        continue;
                    }
                }
            }
            // `normal.w = 127`: the render side drops the quad while it faces away.
            let packed = if normal == glam::Vec3::ZERO {
                [0; 4]
            } else {
                let n = normal * 127.0;
                [n.x.round() as i8, n.y.round() as i8, n.z.round() as i8, 127]
            };
            for v in q {
                let mut v = *v;
                v.normal = packed;
                kept.push(v);
            }
        }
        quads = kept;
    });
    quads
}

// ---------------------------------------------------------------- printer

/// Blocks whose use would open a screen or change them, which the printer must not click.
fn interactive(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "minecraft:crafting_table", "minecraft:crafter", "minecraft:lever", "minecraft:repeater", "minecraft:comparator",
        "minecraft:note_block", "minecraft:daylight_detector", "minecraft:anvil", "minecraft:chipped_anvil",
        "minecraft:damaged_anvil", "minecraft:enchanting_table", "minecraft:grindstone", "minecraft:loom",
        "minecraft:cartography_table", "minecraft:smithing_table", "minecraft:stonecutter", "minecraft:cake",
        "minecraft:bell", "minecraft:respawn_anchor", "minecraft:composter", "minecraft:cauldron",
        "minecraft:water_cauldron", "minecraft:lava_cauldron", "minecraft:jukebox", "minecraft:lectern",
        "minecraft:redstone_wire", "minecraft:dragon_egg", "minecraft:beacon", "minecraft:flower_pot",
        "minecraft:chiseled_bookshelf", "minecraft:decorated_pot", "minecraft:vault", "minecraft:trial_spawner",
    ];
    EXACT.contains(&name)
        || ["_door", "_trapdoor", "_fence_gate", "_button", "_bed", "_sign", "_hanging_sign", "_shulker_box", "chest", "barrel", "furnace", "smoker", "hopper", "dispenser", "dropper", "potted_", "candle", "brewing_stand", "_cake"]
            .iter()
            .any(|p| name.contains(p))
}

fn face_dir(f: [i32; 3]) -> Direction {
    match f {
        [0, 1, 0] => Direction::Up,
        [0, -1, 0] => Direction::Down,
        [0, 0, -1] => Direction::North,
        [0, 0, 1] => Direction::South,
        [-1, 0, 0] => Direction::West,
        _ => Direction::East,
    }
}

/// What the printer does at a spot.
#[derive(Clone, Debug)]
enum Job {
    /// Place the missing block.
    Place,
    /// Click the block that is there with more of its item (half slab to double, candles...).
    Stack,
    /// Click the placed block this many times to set it (repeater delay, comparator mode...).
    Adjust(u32),
}

/// A click the printer makes: the look the server is told, where it lands, and whether it is
/// made sneaking (placing against a chest or door, or a chest that mustn't join its neighbour).
struct Click {
    yaw: f32,
    pitch: f32,
    hit: super::Hit,
    sneak: bool,
}

fn printer_step(game: &mut Game<'_>) {
    let now = game.ticks;
    // On a server, the click went out last tick: give it the player's real look back.
    if with(|s| std::mem::take(&mut s.server_look)) {
        game.forced_hit = None;
        let (yaw, pitch) = (game.yaw, game.pitch);
        send_look(game, yaw, pitch);
    }
    let (on, settings) = with(|s| (s.settings.printer_on && s.slots.iter().any(|x| x.shown) && !s.moving, s.settings.printer));
    if !on || !game.native_playing() || now < with(|s| s.next_print) {
        return;
    }
    let Some(eye) = game.eye_position() else { return };
    let reach = settings.reach.min(game.reach());
    let layer = with(|s| s.layer);
    let feet = [eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32];

    // Everything there is to do in reach, bottom up and nearest first...
    let mut jobs: Vec<(u8, bool, i32, bool, usize, [i32; 3], BlockState, Job)> = Vec::new();
    let held = game.inventory.get(game.selected).and_then(Option::as_ref).map(|st| st.item.to_string());
    let mut waiting = None;
    let mut unsupported = 0usize;
    let mut pillar = None;
    let mut stuck = 0usize;
    for (order, p) in printer::positions_in_reach(eye, reach).into_iter().enumerate() {
        if !layer.contains(p[1]) || !loaded(game, p) || with(|s| s.recent.blocked(p, now, settings.retry_ticks)) {
            continue;
        }
        let Some((status, wanted)) = with(|s| status_at(game, s, p)) else { continue };
        if wanted.is_air() || matches!(wanted.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column") {
            continue;
        }
        let mut needs_air = false;
        let job = match status {
            Status::Missing => {
                // The other half of a door or bed comes with its first half.
                if materials::items_for_state(&wanted) == 0 || p == feet || p == [feet[0], feet[1] + 1, feet[2]] {
                    continue;
                }
                // Sand, gravel, anvils...: only onto something, or they fall.
                if printer::falls(&wanted) {
                    let below = [p[0], p[1] - 1, p[2]];
                    let id = game.world.block(below[0], below[1], below[2]);
                    let facts = game.res.facts();
                    let f = facts.get(id);
                    let free = facts.is_air(id) || f.replaceable || (f.fluid != horizon_client::blocks::Fluid::None && !f.waterlogged);
                    if free {
                        let comes = with(|s| status_at(game, s, below)).is_some_and(|(_, w)| !w.is_air());
                        waiting = Some(if comes {
                            format!("{} at {} {} {} waits for the block below", short(&wanted.name), p[0], p[1], p[2])
                        } else {
                            format!("{} at {} {} {} would fall: nothing below in the schematic", short(&wanted.name), p[0], p[1], p[2])
                        });
                        continue;
                    }
                }
                // An observer waits for the block it watches, or placing that would fire it;
                // not when that block couldn't be placed (it may need the observer to stand on,
                // like a repeater on an observer looking up).
                if let Some(d) = printer::observed(&wanted) {
                    let watched = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    let stuck = with(|s| s.recent.blocked(watched, now, settings.retry_ticks));
                    if !stuck
                        && with(|s| status_at(game, s, watched)).is_some_and(|(st, w)| st == Status::Missing && !w.is_air())
                    {
                        waiting = Some(format!("observer at {} {} {} waits for what it watches", p[0], p[1], p[2]));
                        continue;
                    }
                }
                // No real block with a face in view: without air place it can only wait for one,
                // so it costs no more than this look.
                needs_air = !has_support(game, eye, reach, p, &wanted);
                if needs_air && !settings.air_place {
                    unsupported += 1;
                    if pillar.is_none()
                        && let Some(axis) = wanted.prop("axis")
                        && has_support(game, eye, reach, p, &BlockState::air())
                    {
                        pillar = Some(format!(
                            "{} at {} {} {} only goes against a block on its {axis} side, with that face in view",
                            short(&wanted.name), p[0], p[1], p[2]
                        ));
                    }
                    continue;
                }
                Job::Place
            }
            Status::WrongState => {
                let actual_id = game.world.block(p[0], p[1], p[2]);
                let actual = with(|s| game_state(game, &mut s.states, actual_id));
                if verify::upgradable(&wanted, &actual) {
                    Job::Stack
                } else if let Some(n) = verify::config_clicks(&wanted, &actual) {
                    Job::Adjust(n)
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        if !matches!(job, Job::Adjust(_))
            && with(|s| {
                let key = stuck_key(game, s, eye, p, &wanted);
                s.stuck.holds(p, key, now)
            })
        {
            stuck += 1;
            continue;
        }
        let other_item = with(|s| item_for(game, s, &wanted)) != held;
        jobs.push((printer::place_class(&wanted), needs_air, p[1], other_item, order, p, wanted, job));
    }
    // ...done in redstone's order: plain blocks, then what carries a signal, then what makes one;
    // within that, what has a real block to click against before what needs air place, bottom up,
    // and in each layer what the hand already holds (a change of hand costs a tick), nearest first.
    jobs.sort_by_key(|(class, needs_air, y, other_item, order, ..)| (*class, *needs_air, *y, *other_item, *order));

    let mut note = waiting.or(pillar).unwrap_or_else(|| {
        if unsupported > 0 {
            format!("{unsupported} block{} wait for a neighbour to click against (air place is off)", if unsupported == 1 { "" } else { "s" })
        } else if stuck > 0 {
            format!("{stuck} block{} can't be placed as wanted from where you stand", if stuck == 1 { "" } else { "s" })
        } else {
            String::new()
        }
    });
    let deadline = std::time::Instant::now() + PRINT_BUDGET;
    for (.., p, wanted, job) in jobs {
        if std::time::Instant::now() > deadline {
            break;
        }
        let item = match &job {
            Job::Adjust(_) => None,
            _ => match with(|s| item_for(game, s, &wanted)) {
                Some(item) => Some(item),
                None => continue,
            },
        };
        if let Some(item) = &item
            && let Err(e) = can_take(game, item)
        {
            note = format!("{}: {e}", short(item));
            with(|s| s.recent.mark(p, now));
            continue;
        }
        // The aim first: it only asks what a click would place, with no need of the item in hand,
        // so a block that can't be placed yet doesn't cost a change of hand and a tick.
        let click = match (&job, &item) {
            (Job::Place, Some(item)) => aim_for(game, eye, reach, p, &wanted, item, settings.air_place),
            (Job::Stack, Some(item)) => aim_stack(game, eye, reach, p, &wanted, item, settings.air_place),
            (Job::Adjust(_), _) => aim_block(game, eye, reach, p),
            _ => None,
        };
        let Some(click) = click else {
            with(|s| {
                let key = stuck_key(game, s, eye, p, &wanted);
                s.stuck.mark(p, key, now);
            });
            note = format!("no way to place {} at {} {} {} as wanted", short(&wanted.name), p[0], p[1], p[2]);
            continue;
        };
        if let Some(item) = &item {
            match select(game, item) {
                Err(e) => {
                    note = format!("{}: {e}", short(item));
                    with(|s| s.recent.mark(p, now));
                    continue;
                }
                // Through a server (also the integrated one of a single-player world) the click
                // would go out before the slot change and place what was in hand before: the
                // change goes out this tick, the click the next.
                Ok(true) if game.remote.is_some() => {
                    with(|s| s.printer_note = format!("taking {} in hand", short(item)));
                    return;
                }
                Ok(_) => {}
            }
        }
        if let Err(e) = do_click(game, &click) {
            with(|s| s.printer_note = format!("click failed: {e}"));
            return;
        }
        let what = match job {
            Job::Place => format!("placed {}", short(&wanted.name)),
            Job::Stack => format!("stacked {}", short(&wanted.name)),
            Job::Adjust(n) => format!("set up {} ({n} click{} left)", short(&wanted.name), if n == 1 { "" } else { "s" }),
        };
        with(|s| {
            // An adjusted block gets its next click soon; on a server once its answer is in.
            if !matches!(job, Job::Adjust(_)) {
                s.placed += 1;
                s.recent.mark(p, now);
            } else if game.remote.is_some() {
                s.recent.mark(p, now);
            }
            s.next_print = now + u64::from(settings.interval.max(1));
            s.printer_note = format!("{what} at {} {} {}", p[0], p[1], p[2]);
        });
        return;
    }
    if !note.is_empty() {
        with(|s| s.printer_note = note);
    }
}

// ---------------------------------------------------------------- easy place

/// Whether easy place has a say right now.
fn easy_active(game: &Game<'_>) -> bool {
    let (on, shown, moving) = with(|s| (s.settings.easy_place, s.slots.iter().any(|x| x.shown), s.moving));
    on && shown && !moving && game.native_playing() && !game.spectator() && !tool_held(game)
}

/// The block the view asks for, for easy place: the player's own two cells can't take one.
fn easy_target(game: &Game<'_>) -> Option<([i32; 3], BlockState)> {
    let eye = game.eye_position()?;
    let feet = [eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32];
    let (cell, status, wanted) = first_wanted(game, &[feet, [feet[0], feet[1] + 1, feet[2]]])?;
    (status == Status::Missing && materials::items_for_state(&wanted) > 0).then_some((cell, wanted))
}

/// Each tick: the game's own held use is stopped before it places a block, the block looked at
/// is taken in hand, and with the button down the next one goes in.
fn easy_place_step(game: &mut Game<'_>) {
    // A screen took the release of the button with it.
    if !game.native_playing() {
        with(|s| {
            s.right_down = false;
            s.easy_taken = false;
        });
        return;
    }
    if !easy_active(game) {
        return;
    }
    if game.right && easy_place_refuses(game) {
        game.right = false;
    }
    if game.using.is_some() || easy_select(game) {
        return;
    }
    let (down, next) = with(|s| (s.right_down, s.easy_next));
    if down && game.ticks >= next {
        easy_place_click(game);
    }
}

/// Takes the block looked at in hand. True when the hand is changing: the click waits a tick, so
/// it can't go out with what was held before.
fn easy_select(game: &mut Game<'_>) -> bool {
    if with(|s| s.settings.printer_on) {
        return false;
    }
    let Some((_, wanted)) = easy_target(game) else { return false };
    let Some(item) = with(|s| item_for(game, s, &wanted)) else { return false };
    let held = game.inventory.get(game.selected).and_then(Option::as_ref).is_some_and(|st| *st.item == *item);
    if held {
        with(|s| s.easy_pick = None);
        return false;
    }
    let now = game.ticks;
    if with(|s| s.easy_pick.as_ref().is_some_and(|(i, until)| *i == item && now < *until)) {
        return true;
    }
    let taken = take_in_hand(game, &item);
    with(|s| match taken {
        Ok(()) => {
            s.easy_pick = Some((item, now + 5));
            s.easy_note.clear();
        }
        Err(e) => {
            s.easy_note = format!("{}: {e}", short(&item));
            s.easy_pick = Some((item, now + 20));
        }
    });
    true
}

/// Places the block the schematic wants where the view first meets one, when it is the item in
/// hand and a click gives exactly that block. True when the click belongs to easy place.
fn easy_place_click(game: &mut Game<'_>) -> bool {
    if !easy_active(game) {
        return false;
    }
    let Some(eye) = game.eye_position() else { return false };
    let Some((p, wanted)) = easy_target(game) else { return false };
    let now = game.ticks;
    let (interval, air, retry) =
        with(|s| (s.settings.easy_interval, s.settings.easy_air, s.settings.printer.retry_ticks));
    // Just clicked and the world hasn't caught up: a second click could make a slab double.
    if with(|s| s.recent.blocked(p, now, retry)) {
        return true;
    }
    let held = game.inventory.get(game.selected).and_then(Option::as_ref).map(|st| st.item.to_string());
    if with(|s| item_for(game, s, &wanted)) != held {
        return false;
    }
    let Some(item) = held else { return false };
    let reach = game.reach();
    let Some(click) = aim_for(game, eye, reach, p, &wanted, &item, air) else {
        // With air place it would go: then nothing but the missing neighbour is in the way.
        let lonely = !air && aim_for(game, eye, reach, p, &wanted, &item, true).is_some();
        let why = if lonely { ": no block next to it (easy air place is off)" } else { " can't go there as wanted from here" };
        with(|s| s.easy_note = format!("{}{why}", short(&wanted.name)));
        return false;
    };
    if do_click(game, &click).is_err() {
        return false;
    }
    with(|s| {
        s.easy_next = now + u64::from(interval.max(1));
        s.placed += 1;
        s.recent.mark(p, now);
        s.easy_note.clear();
    });
    true
}

/// With easy place active, whether this click would make the game place a block on its own
/// (asked of the game's placement, so food, buckets and an empty hand don't count). With nothing
/// to click yet a block in hand counts too: the aim can move before the game gets to the press.
/// Using a chest, door or lever stays possible; Shift places against them, so not then.
fn easy_place_refuses(game: &mut Game<'_>) -> bool {
    use winit::keyboard::KeyCode;
    if !easy_active(game) {
        return false;
    }
    let Some(item) = game.inventory.get(game.selected).and_then(Option::as_ref).map(|st| st.item.to_string()) else { return false };
    let Some((origin, dir)) = game.aim() else { return false };
    let reach = game.reach();
    let Some(hit) = game.clip_ray(origin, dir, reach) else {
        return game.pack.assets.registry.default_state(&item).is_some();
    };
    let sneaking = game.keys.contains(&KeyCode::ShiftLeft) || game.keys.contains(&KeyCode::ShiftRight);
    let clicked = game.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
    if !sneaking && with(|s| interactive(&game_state(game, &mut s.states, clicked).name)) {
        return false;
    }
    let ctx = PlaceContext {
        clicked_pos: BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
        face: face_dir(hit.face),
        hit: Vec3::new(hit.point[0], hit.point[1], hit.point[2]),
        inside: false,
        yaw: game.yaw,
        pitch: game.pitch,
        sneaking,
        item,
        can_use_game_master_blocks: game.can_use_game_master_blocks(),
    };
    game.with_level(|b, level, ticks| b.placement_state(level, ticks, &ctx)).is_some()
}

/// Makes `click`: the camera never moves (like litematica-printer, only the click carries the
/// look). Locally it is used within this tick, so the view is back before the next frame; on a
/// server the look is sent just before it and the real one the next tick.
fn do_click(game: &mut Game<'_>, click: &Click) -> Result<(), String> {
    use winit::keyboard::KeyCode;
    if game.remote.is_some() {
        send_look(game, click.yaw, click.pitch);
        game.forced_hit = Some(click.hit);
        with(|s| s.own_click = true);
        let _ = game.play_mouse(winit::event::MouseButton::Right, true);
        let _ = game.play_mouse(winit::event::MouseButton::Right, false);
        with(|s| s.own_click = false);
        with(|s| s.server_look = true);
        return Ok(());
    }
    let before = (game.yaw, game.pitch);
    // Sneaking is read from the held Shift key: held for a sneaking click, let go otherwise
    // (a player holding Shift would place against chests instead of setting a repeater).
    let shift_held = game.keys.contains(&KeyCode::ShiftLeft);
    if click.sneak {
        game.keys.insert(KeyCode::ShiftLeft);
    } else {
        game.keys.remove(&KeyCode::ShiftLeft);
    }
    game.yaw = click.yaw;
    game.pitch = click.pitch;
    game.forced_hit = Some(click.hit);
    let used = game.use_with_hand();
    game.forced_hit = None;
    game.use_delay = 4;
    game.yaw = before.0;
    game.pitch = before.1;
    if shift_held {
        game.keys.insert(KeyCode::ShiftLeft);
    } else {
        game.keys.remove(&KeyCode::ShiftLeft);
    }
    used.map_err(|e| e.to_string())
}

/// Tells the server the player looks along `yaw`/`pitch` (`ServerboundMovePlayerPacket.Rot`).
fn send_look(game: &Game<'_>, yaw: f32, pitch: f32) {
    use horizon_protocol::packets::play::{self, Serverbound as S};
    let (on_ground, collision) =
        game.player.as_ref().map_or((false, false), |p| (p.ground.on_ground, p.collision.horizontal));
    game.send(S::MovePlayerRot(play::MovePlayerRot {
        yaw,
        pitch,
        flags: u8::from(on_ground) | u8::from(collision) << 1,
    }));
}

/// Whether `item` can be put in hand: it is in the hotbar, or the player is in creative.
fn can_take(game: &Game<'_>, item: &str) -> Result<(), String> {
    if game.creative || (0..9).any(|i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) {
        return Ok(());
    }
    Err(if game.inventory.iter().flatten().any(|s| &*s.item == item) {
        "in the inventory but not the hotbar".into()
    } else {
        "none carried".into()
    })
}

/// Whether the block at `support` can be clicked to place against it. Sneaking lets chests, doors
/// and the like be clicked without opening them.
fn clickable(game: &Game<'_>, support: [i32; 3], sneak: bool) -> bool {
    let sid = game.world.block(support[0], support[1], support[2]);
    let facts = game.res.facts();
    let sf = facts.get(sid);
    !(facts.is_air(sid)
        || sf.replaceable
        || sf.outline.is_empty()
        || (!sneak && interactive(&sf.name)))
}

/// Whether `p` is empty, so a click can only come through one of its neighbours' faces (grass or
/// snow there can be clicked itself).
fn is_open(game: &Game<'_>, p: [i32; 3]) -> bool {
    game.res.facts().is_air(game.world.block(p[0], p[1], p[2]))
}

/// Whether a real neighbour of `p` shows the eye a face to click (`aim_for` finds out whether it
/// gives the state wanted). On a server the click isn't made sneaking.
fn has_support(game: &Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState) -> bool {
    let sneak = game.remote.is_none();
    let open = is_open(game, p);
    printer::FACES.iter().any(|&f| {
        (!open || (printer::face_fits(wanted, f) && printer::face_in_view(eye, reach, p, f)))
            && clickable(game, [p[0] + f[0], p[1] + f[1], p[2] + f[2]], sneak)
    })
}

/// What decides whether a block can be placed from here: where the eye is (to a quarter block), the
/// spot and its neighbours, and what is wanted there.
fn stuck_key(game: &Game<'_>, s: &mut State, eye: [f64; 3], p: [i32; 3], wanted: &BlockState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    eye.map(|v| (v * 4.0).floor() as i64).hash(&mut h);
    game.world.block(p[0], p[1], p[2]).0.hash(&mut h);
    for f in printer::FACES {
        game.world.block(p[0] + f[0], p[1] + f[1], p[2] + f[2]).0.hash(&mut h);
    }
    wanted_id(game, s, wanted).map(|id| id.0).hash(&mut h);
    h.finish()
}

/// Puts `item` in hand from the hotbar. In creative only the hotbar is used: an empty slot, or
/// one whose item the schematic doesn't need any more, or the one the printer used longest ago,
/// gets the item (the rest of the inventory stays as it is). Whether the hand changed.
fn select(game: &mut Game<'_>, item: &str) -> Result<bool, String> {
    let now = game.ticks;
    can_take(game, item)?;
    if let Some(i) = (0..9).find(|&i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) {
        let changed = game.selected != i;
        game.selected = i;
        with(|s| s.hotbar_used[i] = now);
        return Ok(changed);
    }
    let needed: std::collections::HashSet<String> = with(|s| {
        let Some(slot) = s.sel() else { return Default::default() };
        let left = if s.tally().is_some() { &s.missing } else { &slot.totals };
        left.iter().filter(|(_, n)| **n > 0).map(|(k, _)| k.clone()).collect()
    });
    let used = with(|s| s.hotbar_used);
    let slot = (0..9)
        .find(|&i| game.inventory[i].is_none())
        .or_else(|| {
            (0..9)
                .filter(|&i| game.inventory[i].as_ref().is_some_and(|s| !needed.contains(&*s.item)))
                .min_by_key(|&i| used[i])
        })
        .or_else(|| (0..9).min_by_key(|&i| used[i]))
        .unwrap_or(0);
    // A creative edit of the slot (a server is told by the inventory sync).
    game.inventory[slot] = Some(horizon_client::items::ItemStack::new(item, 1));
    game.selected = slot;
    with(|s| s.hotbar_used[slot] = now);
    Ok(true)
}

/// Asks the game's own placement what a click would place; whether that is (on the way to)
/// `wanted` at `p`.
#[allow(clippy::too_many_arguments)]
fn would_place(
    game: &mut Game<'_>,
    p: [i32; 3],
    wanted: &BlockState,
    wanted_id: BlockStateId,
    item: &str,
    hit: &super::Hit,
    yaw: f32,
    pitch: f32,
    sneak: bool,
) -> bool {
    let ctx = PlaceContext {
        clicked_pos: BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
        face: face_dir(hit.face),
        hit: Vec3::new(hit.point[0], hit.point[1], hit.point[2]),
        inside: false,
        yaw,
        pitch,
        sneaking: sneak,
        item: item.to_string(),
        can_use_game_master_blocks: game.can_use_game_master_blocks(),
    };
    let Some((pos, state)) = game.with_level(|b, level, ticks| b.placement_state(level, ticks, &ctx)) else {
        return false;
    };
    if [pos.x, pos.y, pos.z] != p {
        return false;
    }
    if state == wanted_id {
        return true;
    }
    let got = with(|s| game_state(game, &mut s.states, state));
    verify::placement_ok(wanted, &got)
}

fn face_axes(f: [i32; 3]) -> (usize, usize, usize) {
    let axis = f.iter().position(|&v| v != 0).unwrap_or(1);
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    (axis, u, v)
}

/// A look and click that make vanilla's placement put `wanted` at `p`: points on each face of
/// the neighbours (sneaking too, when not on a server: then chests and doors can be clicked,
/// and a chest stays single), asking the game's own `placement_state` what each would place.
/// With `air`, a spot no neighbour can be clicked for is clicked itself ("air place": the
/// clicked block is replaceable, so `BlockPlaceContext` places into it), with any look.
fn aim_for(game: &mut Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState, item: &str, air: bool) -> Option<Click> {
    let wanted_id = with(|s| wanted_id(game, s, wanted))?;
    const OFFSETS: [f64; 5] = [0.0, -0.3, 0.3, -0.42, 0.42];
    let sneaks: &[bool] = if game.remote.is_some() { &[false] } else { &[false, true] };
    let open = is_open(game, p);
    for &sneak in sneaks {
        for f in printer::FACES {
            let support = [p[0] + f[0], p[1] + f[1], p[2] + f[2]];
            if (open && !(printer::face_fits(wanted, f) && printer::face_in_view(eye, reach, p, f))) || !clickable(game, support, sneak) {
                continue;
            }
            // The face of `support` that touches `p`.
            let centre = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 + f64::from(f[i]) * 0.5);
            let (_, u, v) = face_axes(f);
            for du in OFFSETS {
                for dv in OFFSETS {
                    let mut point = centre;
                    point[u] += du;
                    point[v] += dv;
                    let (yaw, pitch) = super::agent::yaw_pitch(eye, point);
                    let pitch = pitch.clamp(-90.0, 90.0);
                    let dir = super::view_vector(yaw, pitch);
                    let Some(hit) = game.clip_ray(eye, dir, reach) else { continue };
                    if hit.pos != support && hit.pos != p {
                        continue;
                    }
                    if would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, sneak) {
                        return Some(Click { yaw, pitch, hit, sneak });
                    }
                }
            }
        }
    }
    if !air {
        return None;
    }
    // Air place: a point on one of `p`'s own faces, looked at naturally first, then any look
    // (stairs, signs and the like take their facing from the look, not the face).
    for &sneak in sneaks {
        for d in printer::FACES {
            let (axis, _, _) = face_axes(d);
            // Off-centre heights on the sides decide top/bottom halves.
            let heights: &[f64] = if axis == 1 { &[0.0] } else { &[0.0, 0.3, -0.3] };
            for &dy in heights {
                let mut point = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 + f64::from(d[i]) * 0.5);
                point[1] += dy;
                let dist = (0..3).map(|i| (point[i] - eye[i]).powi(2)).sum::<f64>().sqrt();
                if dist > reach {
                    continue;
                }
                let hit = super::Hit { pos: p, face: d, point };
                let (ny, np) = super::agent::yaw_pitch(eye, point);
                let natural = std::iter::once((ny, np.clamp(-90.0, 90.0)));
                let any = (0..16).flat_map(|i| {
                    [0.0f32, 50.0, -50.0, 90.0, -90.0].map(move |pitch| (i as f32 * 22.5 - 180.0, pitch))
                });
                for (yaw, pitch) in natural.chain(any) {
                    if would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, sneak) {
                        return Some(Click { yaw, pitch, hit, sneak });
                    }
                }
            }
        }
    }
    None
}

/// A click on the block at `p` itself with more of its item, that makes it `wanted` (the top
/// half onto a bottom slab, another candle...): looked at through its own shape, else (with
/// `air`) aimed straight at a face.
fn aim_stack(game: &mut Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState, item: &str, air: bool) -> Option<Click> {
    let wanted_id = with(|s| wanted_id(game, s, wanted))?;
    const OFFSETS: [f64; 3] = [0.0, -0.3, 0.3];
    for f in printer::FACES {
        let centre = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 + f64::from(f[i]) * 0.5);
        let (_, u, v) = face_axes(f);
        for du in OFFSETS {
            for dv in OFFSETS {
                let mut point = centre;
                point[u] += du;
                point[v] += dv;
                let (yaw, pitch) = super::agent::yaw_pitch(eye, point);
                let pitch = pitch.clamp(-90.0, 90.0);
                let dir = super::view_vector(yaw, pitch);
                if let Some(hit) = game.clip_ray(eye, dir, reach)
                    && hit.pos == p
                    && would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, false)
                {
                    return Some(Click { yaw, pitch, hit, sneak: false });
                }
                if air {
                    let hit = super::Hit { pos: p, face: f, point };
                    if would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, false) {
                        return Some(Click { yaw, pitch, hit, sneak: false });
                    }
                }
            }
        }
    }
    None
}

/// A plain (not sneaking) click on the block at `p`, to use it: a repeater's delay, a
/// comparator's mode, a note block's note.
fn aim_block(game: &mut Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3]) -> Option<Click> {
    let centre = [f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.1, f64::from(p[2]) + 0.5];
    let (yaw, pitch) = super::agent::yaw_pitch(eye, centre);
    let pitch = pitch.clamp(-90.0, 90.0);
    let dir = super::view_vector(yaw, pitch);
    let hit = match game.clip_ray(eye, dir, reach) {
        Some(hit) if hit.pos == p => hit,
        _ => super::Hit { pos: p, face: [0, 1, 0], point: centre },
    };
    Some(Click { yaw, pitch, hit, sneak: false })
}

// ---------------------------------------------------------------- the selection tool

/// What the selection tool's wheel moves (Litematica's selected corner or box).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Grab {
    /// The whole box: both corners.
    #[default]
    Box,
    /// One corner (0 or 1).
    Corner(usize),
}

/// How far middle click looks for a corner.
const PICK_RANGE: f64 = 512.0;

/// Whether the selection tool is in the main hand (and switched on).
fn tool_held(game: &Game<'_>) -> bool {
    let item = with(|s| {
        load_settings(game, s);
        s.settings.tool.then(|| s.settings.tool_item.clone())
    });
    item.is_some_and(|item| game.inventory.get(game.selected).and_then(Option::as_ref).is_some_and(|st| *st.item == *item))
}

/// Where a tool click puts a corner: the first block along the view within the tool's reach
/// (water and lava count when the setting says so), or the block in front of the face hit.
fn tool_target(game: &Game<'_>) -> Option<[i32; 3]> {
    let (range, front, fluids) = with(|s| (s.settings.tool_range, s.settings.tool_front, s.settings.tool_fluids));
    let (origin, dir) = game.aim()?;
    let hit = game.clip_ray(origin, dir, range).map(|h| {
        let length = (0..3).map(|i| (h.point[i] - origin[i]).powi(2)).sum::<f64>().sqrt();
        (h.pos, [0, 1, 2].map(|i| h.pos[i] + h.face[i]), length)
    });
    let hit = match fluids.then(|| first_fluid(game, origin, dir, hit.map_or(range, |h| h.2))).flatten() {
        Some((cell, before)) => (cell, before),
        None => hit.map(|h| (h.0, h.1))?,
    };
    Some(if front { hit.1 } else { hit.0 })
}

/// The first water or lava cell along the ray within `reach`, and the cell before it.
fn first_fluid(game: &Game<'_>, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<([i32; 3], [i32; 3])> {
    let cell_at = |t: f64| [0, 1, 2].map(|i| (origin[i] + dir[i] * t).floor() as i32);
    let mut last = cell_at(0.0);
    let mut t = 0.0;
    while t < reach {
        t += 0.05;
        let cell = cell_at(t);
        if cell == last {
            continue;
        }
        let id = game.world.block(cell[0], cell[1], cell[2]);
        let fluid = with(|s| {
            let st = game_state(game, &mut s.states, id);
            matches!(st.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column")
                || st.prop("waterlogged") == Some("true")
        });
        if fluid {
            return Some((cell, last));
        }
        last = cell;
    }
    None
}

/// Where the ray from `o` along `d` enters the box `lo..hi`, if it does.
fn ray_box(o: [f64; 3], d: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> Option<f64> {
    let (mut near, mut far) = (0.0f64, PICK_RANGE);
    for i in 0..3 {
        if d[i].abs() < 1e-9 {
            if o[i] < lo[i] || o[i] > hi[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// A mouse button while playing, with the selection tool in hand: left click puts corner 1 on
/// the block looked at, right click corner 2, middle click picks what the wheel moves (the
/// corner looked at, else the whole box). The game doesn't see these clicks (nothing is broken
/// or used). Whether the click was the tool's.
pub(super) fn mouse(game: &mut Game<'_>, button: winit::event::MouseButton, pressed: bool) -> bool {
    use winit::event::MouseButton;
    let b = match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        _ => return false,
    };
    // The mod's own clicks come through here too: those go to the game.
    if with(|s| s.own_click) {
        return false;
    }
    if b == 1 {
        with(|s| s.right_down = pressed);
    }
    // A release goes where its press went.
    if !pressed {
        if b == 1 && with(|s| std::mem::take(&mut s.easy_taken)) {
            return true;
        }
        return with(|s| std::mem::take(&mut s.tool_down[b]));
    }
    if !game.native_on(ID) || !game.native_playing() {
        return false;
    }
    // Easy place: the press is taken when it places, and when the game's own use would place
    // a block the schematic doesn't want there.
    if b == 1 {
        let taken = !tool_held(game) && (easy_place_click(game) || easy_place_refuses(game));
        with(|s| s.easy_taken = taken);
        if taken {
            return true;
        }
    }
    if !tool_held(game) {
        // Middle click on a block the schematic still wants: that block, as the game's own pick.
        if b == 2 && pick_wanted(game) {
            with(|s| s.tool_down[b] = true);
            return true;
        }
        return false;
    }
    with(|s| s.tool_down[b] = true);
    // Ctrl + middle click: the other mode.
    if b == 2 && game.modifiers.control_key() {
        let r = run(game, &["tool", "mode", "toggle"]);
        report(game, r);
        return true;
    }
    if with(|s| s.settings.tool_placement) {
        placement_click(game, b);
        return true;
    }
    if b < 2 {
        if let Some(at) = tool_target(game) {
            with(|s| {
                s.area[b] = Some(at);
                s.grab = Grab::Corner(b);
            });
        }
        return true;
    }
    let Some((origin, dir)) = game.aim() else { return true };
    with(|s| {
        let corner = |i: usize| {
            let c = s.area[i]?.map(f64::from);
            ray_box(origin, dir, c.map(|v| v - 0.05), c.map(|v| v + 1.05)).map(|t| (t, i))
        };
        let looked = [corner(0), corner(1)].into_iter().flatten().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, i)| i);
        s.grab = match looked {
            // The corner already picked, picked again: back to the box.
            Some(i) if s.grab != Grab::Corner(i) => Grab::Corner(i),
            _ => Grab::Box,
        };
    });
    true
}

/// The first block along the view within reach that a shown placement wants and the world doesn't
/// have yet, up to and with the first real block: where it is, how it stands, what is wanted. The
/// cell the eye is in doesn't count, the player is inside that one; nor do the cells in `skip`.
fn first_wanted(game: &Game<'_>, skip: &[[i32; 3]]) -> Option<([i32; 3], Status, BlockState)> {
    let (origin, dir) = game.aim()?;
    let reach = game.reach();
    let limit = game
        .clip_ray(origin, dir, reach)
        .map_or(reach, |h| (0..3).map(|i| (h.point[i] - origin[i]).powi(2)).sum::<f64>().sqrt());
    let layer = with(|s| s.layer);
    let cell_at = |t: f64| [0, 1, 2].map(|i| (origin[i] + dir[i] * t).floor() as i32);
    let mut last = cell_at(0.0);
    let mut t = 0.0;
    while t < limit {
        t = (t + 0.02).min(limit);
        let cell = cell_at(t);
        if cell == last {
            continue;
        }
        last = cell;
        if !layer.contains(cell[1]) || skip.contains(&cell) {
            continue;
        }
        let Some((status, wanted)) = with(|s| status_at(game, s, cell)) else { continue };
        if status != Status::Correct
            && !wanted.is_air()
            && !matches!(wanted.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column")
        {
            return Some((cell, status, wanted));
        }
    }
    None
}

fn wanted_in_view(game: &Game<'_>) -> Option<BlockState> {
    first_wanted(game, &[]).map(|(.., wanted)| wanted)
}

/// Takes the block the schematic wants where the player looks, like the game's own pick block: from
/// the hotbar, from the inventory into the hotbar, or in creative out of nothing. Whether the click
/// was taken: not with the overlay off, nor when it isn't on such a block (the game's own pick).
fn pick_wanted(game: &mut Game<'_>) -> bool {
    if game.spectator() || !with(|s| s.settings.render && s.slots.iter().any(|x| x.shown)) || game.pick_entity_hit().is_some() {
        return false;
    }
    let Some(wanted) = wanted_in_view(game) else { return false };
    let Some(item) = with(|s| item_for(game, s, &wanted)) else { return false };
    if let Err(e) = take_in_hand(game, &item) {
        say(game, &format!("{}: {e}", short(&item)));
    }
    true
}

/// Puts `item` in hand as the game's pick block does (`pick_item`), on a server with the click that
/// swaps a stack of the inventory into the hotbar.
fn take_in_hand(game: &mut Game<'_>, item: &str) -> Result<(), String> {
    use horizon_client::items::ItemStack;
    game.select_queued_hotbar();
    if let Some(slot) = (0..9).find(|&i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) {
        game.selected = slot;
        return Ok(());
    }
    if game.creative {
        horizon_client::pick_item::pick(&mut game.inventory[..36], &mut game.selected, &ItemStack::new(item, 1), true);
        return Ok(());
    }
    let Some(slot) = (9..36).find(|&i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) else {
        return Err("none carried".into());
    };
    // `getSuitableHotbarSlot`: from the selected slot on, an empty one, else one without enchantments.
    let from = game.selected;
    let order = (0..9).map(|i| (from + i) % 9);
    let hotbar = order
        .clone()
        .find(|&i| game.inventory[i].is_none())
        .or_else(|| order.clone().find(|&i| game.inventory[i].as_ref().is_some_and(|s| s.enchantments().is_empty())))
        .unwrap_or(from);
    if game.remote.is_some() {
        if !game.remote_menu_ops(&[(Some(slot), hotbar as i32, horizon_client::menu::Input::Swap)]) {
            return Err("the inventory can't be changed right now".into());
        }
    } else {
        game.inventory.swap(hotbar, slot);
    }
    game.selected = hotbar;
    Ok(())
}

/// The placement looked at: the nearest box the view enters (inside two, the smaller).
fn looked_placement(game: &Game<'_>) -> Option<usize> {
    let (origin, dir) = game.aim()?;
    with(|s| {
        s.slots
            .iter()
            .enumerate()
            .filter(|(_, x)| x.shown)
            .filter_map(|(i, x)| {
                let (lo, hi) = x.placement.bounds();
                let t = ray_box(origin, dir, lo.map(f64::from), hi.map(|v| f64::from(v) + 1.0))?;
                let volume: i64 = (0..3).map(|k| i64::from(hi[k] - lo[k] + 1)).product();
                Some((t, volume, i))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(.., i)| i)
    })
}

/// A click with the tool in placement mode: left (or middle) selects the placement looked at,
/// right puts the selected one's origin on the block looked at.
fn placement_click(game: &mut Game<'_>, b: usize) {
    if b == 1 {
        let Some(at) = tool_target(game) else { return };
        let r = run(game, &["origin", &at[0].to_string(), &at[1].to_string(), &at[2].to_string()]);
        if let Err(e) = r {
            say(game, &format!("Error: {e}"));
        }
        return;
    }
    match looked_placement(game) {
        Some(i) if with(|s| s.selected != i) => {
            let r = run(game, &["select", &(i + 1).to_string()]);
            if let Err(e) = r {
                say(game, &format!("Error: {e}"));
            }
        }
        Some(_) => {}
        None => say(game, "No placement there (look at one's box)"),
    }
}

/// The wheel with the tool in placement mode: Alt + wheel moves the selected placement along
/// the axis you look along, Ctrl + wheel turns it (up: clockwise). Shift: 5 blocks.
fn placement_wheel(game: &mut Game<'_>, steps: f32, claimed: &mut bool) {
    let (alt, ctrl) = (game.modifiers.alt_key(), game.modifiers.control_key());
    if !(alt || ctrl) || with(|s| s.placement().is_none()) {
        return;
    }
    *claimed = true;
    let n = with(|s| {
        s.wheel += steps;
        let n = s.wheel.trunc();
        s.wheel -= n;
        n as i32
    });
    if n == 0 {
        return;
    }
    if ctrl {
        for _ in 0..n.unsigned_abs() {
            if let Err(e) = run(game, &["rotate", if n > 0 { "cw" } else { "ccw" }]) {
                say(game, &format!("Error: {e}"));
                break;
            }
        }
        return;
    }
    let axis = dominant(super::view_vector(game.yaw, game.pitch));
    let k = n * move_step(game);
    if shift(axis.map(|v| v * k)) {
        save_placements(game);
    } else {
        say(game, "The placement is locked (/hm unlock)");
    }
}

/// The wheel with the selection tool in hand: Alt + wheel moves the picked corner (or the whole
/// box) along the axis you look along, Ctrl + wheel pushes the box's face in that direction out
/// (wheel up) or in (wheel down). Shift: 5 blocks. Without Alt or Ctrl the wheel is the hotbar's.
fn tool_wheel(game: &mut Game<'_>, steps: f32, claimed: &mut bool) {
    let (alt, ctrl) = (game.modifiers.alt_key(), game.modifiers.control_key());
    if !(alt || ctrl) || !tool_held(game) {
        return;
    }
    if with(|s| s.settings.tool_placement) {
        placement_wheel(game, steps, claimed);
        return;
    }
    if with(|s| s.area.iter().all(Option::is_none)) {
        return;
    }
    *claimed = true;
    let n = with(|s| {
        s.wheel += steps;
        let n = s.wheel.trunc();
        s.wheel -= n;
        n as i32
    });
    if n == 0 {
        return;
    }
    let axis = dominant(super::view_vector(game.yaw, game.pitch));
    let k = n * move_step(game);
    with(|s| {
        if ctrl {
            grow(s, axis, k);
        } else {
            let moved = |c: Option<[i32; 3]>| c.map(|c| [0, 1, 2].map(|i| c[i] + axis[i] * k));
            match s.grab {
                Grab::Corner(i) => s.area[i] = moved(s.area[i]),
                Grab::Box => s.area = s.area.map(moved),
            }
        }
    });
}

/// Pushes the face of the box that `axis` points at by `k` blocks: out when `k` is positive, in
/// when negative (never past the opposite face).
fn grow(s: &mut State, axis: [i32; 3], k: i32) {
    let [Some(a), Some(b)] = s.area else { return };
    let i = (0..3).find(|&i| axis[i] != 0).unwrap_or(0);
    let sign = axis[i];
    // The corner on that face (the far one along `axis`), and the other one.
    let f = if (a[i] - b[i]) * sign > 0 || (a[i] == b[i] && sign > 0) { 0 } else { 1 };
    let (mut c, other) = if f == 0 { (a, b[i]) } else { (b, a[i]) };
    c[i] += sign * k;
    // Shrunk to one block at most.
    if (c[i] - other) * sign < 0 {
        c[i] = other;
    }
    s.area[f] = Some(c);
}

/// The HUD's lines while the selection tool is in hand.
fn tool_lines(s: &State) -> Vec<(String, u32)> {
    if s.settings.tool_placement {
        let selected = match s.sel() {
            Some(slot) => {
                let p = &slot.placement;
                format!(
                    "Selected: {} at {} {} {} · {}{}",
                    display_name(p),
                    p.origin[0],
                    p.origin[1],
                    p.origin[2],
                    p.rotation.name(),
                    if slot.locked { " · LOCKED" } else { "" }
                )
            }
            None => "No placement loaded".into(),
        };
        return vec![
            ("Placement tool: left click selects the placement you look at · right click moves it there".into(), 0x55FFFF),
            (selected, 0xE0E0E0),
            ("Alt+wheel moves it · Ctrl+wheel turns it · Shift: 5 · Ctrl+middle click: area mode".into(), 0xAAAAAA),
        ];
    }
    let c = |i: usize| s.area[i].map_or("not set".to_string(), |c| format!("{} {} {}", c[0], c[1], c[2]));
    let mut corners = format!("Corner 1: {} · corner 2: {}", c(0), c(1));
    if let Some((lo, hi)) = s.area_box() {
        corners += &format!(" · {} x {} x {}", hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1);
    }
    let what = match s.grab {
        Grab::Box => "the box".to_string(),
        Grab::Corner(i) => format!("corner {}", i + 1),
    };
    vec![
        ("Selection tool: left click corner 1 · right click corner 2 · middle click picks a corner".into(), 0xFFFF55),
        (corners, 0xE0E0E0),
        (format!("Alt+wheel moves {what} · Ctrl+wheel grows/shrinks where you look · Shift: 5 · Ctrl+middle click: placement mode"), 0xAAAAAA),
    ]
}

// ---------------------------------------------------------------- saving an area

/// Most positions one save takes: as many as a file may hold to be loaded again (2^31).
const MAX_SAVE: u64 = promatica_core::schematic::MAX_VOLUME;
/// Time a save may take per tick.
const SAVE_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);
/// The render side's section key for the frames around the chunks a save still waits for.
const SAVE_SECTION: [i32; 3] = [i32::MIN, i32::MIN, i32::MIN + 1];
/// Most of those frames drawn (the nearest).
const SAVE_FRAMES: usize = 1024;

/// A save under way: the area is read chunk by chunk as the chunks are loaded (as Litematica's
/// task does), a few per tick; each chunk as it was when it was read.
struct SaveJob {
    /// The name given, the file name, and where it goes.
    name: String,
    file: String,
    path: std::path::PathBuf,
    lo: [i32; 3],
    hi: [i32; 3],
    builder: promatica_core::schematic::RegionBuilder,
    /// Game state id → palette index.
    ids: HashMap<u32, u32>,
    /// Chunk columns not read yet.
    pending: std::collections::BTreeSet<(i32, i32)>,
    total: usize,
    /// Of `pending`, those not loaded, and the nearest of them (updated every tick).
    unloaded: Vec<(i32, i32)>,
    started: std::time::Instant,
    /// The world (`world_key`) the area is in.
    world: Option<String>,
}

/// A file being written by its own thread: its name and the line it reports when done.
struct SaveWrite {
    file: String,
    done: std::sync::mpsc::Receiver<String>,
}

/// The file name (without `.litematic`) for a schematic called `name`: characters no file system
/// takes become `_`.
fn file_stem(name: &str) -> Result<String, String> {
    let name = name.trim();
    let name = if name.to_ascii_lowercase().ends_with(".litematic") { &name[..name.len() - 10] } else { name };
    let stem: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let stem = stem.trim_end_matches(['.', ' ']).trim().to_string();
    if stem.is_empty() {
        return Err("give it a name".into());
    }
    if stem.chars().count() > 100 {
        return Err("that name is too long".into());
    }
    Ok(stem)
}

/// Starts saving the area selection as `<name>.litematic` in the schematics folder: one region
/// with every block and its state, as Litematica writes it (version 7). The chunks are read as
/// they are loaded; the file is written once the last one is in. Block entity data (chest
/// contents, sign texts) and entities aren't known to the client this way and aren't saved.
fn start_save(game: &mut Game<'_>, name: &str, overwrite: bool) -> Result<String, String> {
    if let Some(file) = with(|s| s.save.as_ref().map(|j| j.file.clone()).or_else(|| s.save_write.as_ref().map(|w| w.file.clone()))) {
        return Err(format!("{file} is still being saved (/hm save cancel stops it)"));
    }
    let (lo, hi) = with(|s| s.area_box()).ok_or("set both corners first (the selection tool, or /hm pos1 and /hm pos2)")?;
    let size = [0, 1, 2].map(|i| hi[i] - lo[i] + 1);
    let volume: u64 = size.iter().map(|&v| v as u64).product();
    if volume > MAX_SAVE {
        return Err(format!("the area has {volume} blocks; at most {MAX_SAVE} can be saved at once"));
    }
    let stem = file_stem(name)?;
    let file = format!("{stem}.litematic");
    let path = schematics_dir()?.join(&file);
    if path.exists() && !overwrite {
        return Err(format!("{file} exists: /hm save {name} overwrite replaces it"));
    }
    let shape = &game.world.shape;
    if lo[1] < shape.min_y || hi[1] >= shape.min_y + shape.height {
        return Err(format!("the area reaches outside the world (y {} to {})", shape.min_y, shape.min_y + shape.height - 1));
    }
    let pending: std::collections::BTreeSet<(i32, i32)> =
        (lo[0] >> 4..=hi[0] >> 4).flat_map(|cx| (lo[2] >> 4..=hi[2] >> 4).map(move |cz| (cx, cz))).collect();
    let total = pending.len();
    let job = SaveJob {
        name: name.trim().to_string(),
        file: file.clone(),
        path,
        lo,
        hi,
        builder: promatica_core::schematic::RegionBuilder::new(size),
        ids: HashMap::new(),
        pending,
        total,
        unloaded: Vec::new(),
        started: std::time::Instant::now(),
        world: world_key(game),
    };
    with(|s| s.save = Some(job));
    Ok(format!(
        "Saving {file}: {} x {} x {} in {total} chunk{} (the HUD shows the progress)",
        size[0],
        size[1],
        size[2],
        if total == 1 { "" } else { "s" }
    ))
}

/// The save's work for this tick: a finished file reported; chunks read while the time lasts;
/// the last one in, the file handed to a thread of its own to write.
fn save_step(game: &mut Game<'_>) {
    if let Some(line) = with(|s| s.save_write.as_ref().and_then(|w| w.done.try_recv().ok())) {
        with(|s| {
            s.save_write = None;
            s.ui_files_at = None;
            s.ui_info.clear();
            s.ui_message = line.clone();
        });
        say(game, &line);
    }
    let Some(mut job) = with(|s| s.save.take()) else { return };
    if job.world != world_key(game) {
        let line = format!("Saving {} stopped: you left the world it is in", job.file);
        with(|s| s.ui_message = line.clone());
        say(game, &line);
        return;
    }
    let start = std::time::Instant::now();
    let loaded: Vec<(i32, i32)> = job.pending.iter().copied().filter(|c| game.world.chunks.get(c).is_some()).collect();
    for c in loaded {
        if start.elapsed() > SAVE_BUDGET {
            break;
        }
        save_chunk(game, &mut job, c);
        job.pending.remove(&c);
    }
    if job.pending.is_empty() {
        finish_save(game, job);
        return;
    }
    // What is still missing, nearest first.
    let me = feet(game).unwrap_or(job.lo);
    let mut unloaded: Vec<(i32, i32)> = job.pending.iter().copied().filter(|c| game.world.chunks.get(c).is_none()).collect();
    unloaded.sort_by_key(|&(cx, cz)| {
        let (dx, dz) = (i64::from(cx * 16 + 8 - me[0]), i64::from(cz * 16 + 8 - me[2]));
        dx * dx + dz * dz
    });
    job.unloaded = unloaded;
    with(|s| s.save = Some(job));
}

/// Reads one chunk column's part of the area. Sections of nothing but air are passed over: every
/// cell is air (index 0) until it is set.
fn save_chunk(game: &Game<'_>, job: &mut SaveJob, (cx, cz): (i32, i32)) {
    let Some(chunk) = game.world.chunks.get(&(cx, cz)) else { return };
    let (lo, hi) = (job.lo, job.hi);
    let (x0, x1) = ((cx * 16).max(lo[0]), (cx * 16 + 15).min(hi[0]));
    let (z0, z1) = ((cz * 16).max(lo[2]), (cz * 16 + 15).min(hi[2]));
    let mut y = lo[1];
    while y <= hi[1] {
        let top = ((y >> 4) * 16 + 15).min(hi[1]);
        let section = chunk.section_index(y).and_then(|i| chunk.sections.get(i));
        if let Some(section) = section.filter(|sec| !sec.is_uniform(0)) {
            for yy in y..=top {
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        let id = section.get(horizon_client::chunk::index(x, yy, z));
                        if id == 0 {
                            continue;
                        }
                        let i = match job.ids.get(&id) {
                            Some(&i) => i,
                            None => {
                                let state = with(|s| game_state(game, &mut s.states, BlockStateId(id)));
                                let i = job.builder.intern(state);
                                job.ids.insert(id, i);
                                i
                            }
                        };
                        if i != 0 {
                            job.builder.set([x - lo[0], yy - lo[1], z - lo[2]], i);
                        }
                    }
                }
            }
        }
        y = top + 1;
    }
}

/// Every chunk is in: the schematic is built and written by a thread of its own (compressing a
/// large one takes a while), first beside its name, so a failed write never leaves half a file.
fn finish_save(game: &mut Game<'_>, job: SaveJob) {
    let SaveJob { name, file, path, lo, hi, builder, started, .. } = job;
    let size = [0, 1, 2].map(|i| hi[i] - lo[i] + 1);
    let volume: i64 = size.iter().map(|&v| i64::from(v)).product();
    let (blocks, states) = (builder.non_air(), builder.states());
    let stem = file.trim_end_matches(".litematic").to_string();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let schematic = Schematic {
        version: 7,
        data_version: i64::from(horizon_world::DATA_VERSION),
        metadata: promatica_core::schematic::Metadata {
            name,
            author: game.options.player_name(),
            description: String::new(),
            region_count: 1,
            total_volume: volume,
            total_blocks: blocks as i64,
            enclosing_size: size,
            time_created: now,
            time_modified: now,
        },
        regions: vec![builder.finish(&stem, Vec::new())],
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let name = file.clone();
    std::thread::spawn(move || {
        let tmp = path.with_file_name(format!("{name}.tmp"));
        let written = std::fs::write(&tmp, schematic.to_bytes())
            .map_err(|e| format!("{}: {e}", tmp.display()))
            .and_then(|()| {
                std::fs::rename(&tmp, &path).map_err(|e| {
                    let _ = std::fs::remove_file(&tmp);
                    format!("{}: {e}", path.display())
                })
            });
        let line = match written {
            Ok(()) => format!(
                "Saved {name}: {} x {} x {}, {blocks} blocks, {states} block states ({:.1} s)",
                size[0],
                size[1],
                size[2],
                started.elapsed().as_secs_f32()
            ),
            Err(e) => format!("Error: couldn't write {name}: {e}"),
        };
        let _ = tx.send(line);
    });
    with(|s| s.save_write = Some(SaveWrite { file, done: rx }));
}

/// Stops the save under way.
fn cancel_save() -> Result<String, String> {
    with(|s| match s.save.take() {
        Some(job) => Ok(format!("Saving {} stopped; nothing was written", job.file)),
        None if s.save_write.is_some() => Err("the file is being written; that can't be stopped".into()),
        None => Err("no save under way".into()),
    })
}

/// "north-east" and the like, for a step of `dx`, `dz` (north is -z).
fn compass(dx: i32, dz: i32) -> &'static str {
    const NAMES: [&str; 8] = ["east", "south-east", "south", "south-west", "west", "north-west", "north", "north-east"];
    let angle = f64::from(dz).atan2(f64::from(dx)).to_degrees().rem_euclid(360.0);
    NAMES[((angle + 22.5) / 45.0) as usize % 8]
}

/// The save's progress: lines for the HUD and `/hm save status`, and how far it is (0 to 1).
fn save_lines(game: &Game<'_>, s: &State) -> Option<(Vec<(String, u32)>, f32)> {
    if let Some(w) = &s.save_write {
        return Some((vec![(format!("Writing {}…", w.file), 0xFFAA00)], 1.0));
    }
    let job = s.save.as_ref()?;
    let done = job.total - job.pending.len();
    let mut lines = vec![(format!("Saving {}: {done} of {} chunks", job.file, job.total), 0xFFAA00)];
    if let Some(&(cx, cz)) = job.unloaded.first() {
        let me = feet(game).unwrap_or(job.lo);
        let (dx, dz) = (cx * 16 + 8 - me[0], cz * 16 + 8 - me[2]);
        let far = f64::from(dx).hypot(f64::from(dz)).round();
        lines.push((
            format!(
                "{} not loaded (orange frames) · nearest: {far} blocks {} ({} {})",
                job.unloaded.len(),
                compass(dx, dz),
                cx * 16 + 8,
                cz * 16 + 8
            ),
            0xFFAA00,
        ));
    }
    Some((lines, done as f32 / job.total.max(1) as f32))
}

/// Hands the frames around the chunks the save still waits for to the render side, when they
/// changed: one orange box per chunk (its part of the area), the nearest `SAVE_FRAMES`.
fn save_frames_step(game: &Game<'_>) {
    let Some((chunks, lo, hi, anchor)) = with(|s| {
        let (chunks, lo, hi) = match &s.save {
            Some(job) => (job.unloaded.iter().take(SAVE_FRAMES).copied().collect::<Vec<_>>(), job.lo, job.hi),
            None => (Vec::new(), [0; 3], [0; 3]),
        };
        let now = (chunks.clone(), s.mesh.anchor);
        (s.mesh.save != Some(now.clone())).then(|| {
            s.mesh.save = Some(now);
            (chunks, lo, hi, s.mesh.anchor)
        })
    }) else {
        return;
    };
    if chunks.is_empty() {
        bridge::remove_section(SAVE_SECTION);
        return;
    }
    let white = horizon_client::gfx::lightning::white_uv(&game.gui.atlas.font.white());
    let mut quads = Vec::new();
    for (cx, cz) in chunks {
        let a = [(cx * 16).max(lo[0]), lo[1], (cz * 16).max(lo[2])];
        let b = [(cx * 16 + 15).min(hi[0]) + 1, hi[1] + 1, (cz * 16 + 15).min(hi[2]) + 1];
        let rel = |p: [i32; 3]| [0, 1, 2].map(|i| (p[i] - anchor[i]) as f32);
        push_frame(&mut quads, white, rel(a), rel(b), 0.08, 0xE0FF_8800);
    }
    bridge::put_section(SAVE_SECTION, quads);
}

// ---------------------------------------------------------------- HUD

fn inventory_counts(game: &Game<'_>) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for s in game.inventory.iter().flatten() {
        *m.entry(s.item.to_string()).or_insert(0) += u64::from(s.count);
    }
    m
}

/// Puts the selected placement's material list on the clipboard, as a table a spreadsheet takes.
fn copy_materials(game: &mut Game<'_>) -> Result<Vec<String>, String> {
    use horizon_client::clipboard::Clipboard;
    let list = material_list(game);
    if list.is_empty() {
        return Err("no schematic loaded, or it needs no items".into());
    }
    game.clipboard.set(&materials::table(&list, pretty_item));
    Ok(vec![format!("Material list copied ({} kinds): paste it into a spreadsheet", list.len())])
}

fn material_list(game: &Game<'_>) -> Vec<Material> {
    let inv = inventory_counts(game);
    with(|s| {
        let Some(slot) = s.sel() else { return Vec::new() };
        let missing = if s.tally().is_some() { s.missing.clone() } else { slot.totals.clone() };
        materials::build_list(&slot.totals, &missing, &inv)
    })
}

pub(super) fn hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_on(ID) {
        return;
    }
    let tool = tool_held(game);
    let lines = with(|s| {
        load_settings(game, s);
        let area = s.area_box().map(|(lo, hi)| [0, 1, 2].map(|i| hi[i] - lo[i] + 1));
        let Some(slot) = s.sel() else {
            // Only an area selection: its size.
            return Some(match area {
                Some(d) if s.settings.hud => vec![(format!("Promatica area: {} x {} x {} (/hm save <name>)", d[0], d[1], d[2]), 0x55FFFFu32)],
                _ => Vec::new(),
            });
        };
        let p = &slot.placement;
        if !s.settings.hud && !s.moving {
            return Some(Vec::new());
        }
        let count = if s.slots.len() > 1 { format!(" ({}/{})", s.selected + 1, s.slots.len()) } else { String::new() };
        let hidden = if slot.shown { "" } else { " · hidden" };
        let mut l = vec![(format!("Promatica: {}{count}{hidden}", display_name(p)), 0x55FFFFu32)];
        if s.moving {
            l.push(("MOVE MODE · wheel/arrows/PgUp/PgDn · R rotate · N done".into(), 0xFFFF55));
        }
        l.push((
            format!(
                "Origin {} {} {} · {} · mirror {}",
                p.origin[0],
                p.origin[1],
                p.origin[2],
                p.rotation.name(),
                p.mirror.name()
            ),
            0xE0E0E0,
        ));
        l.push((
            format!("Layers: {}{}", s.layer.describe(), if s.settings.render { "" } else { " (hidden)" }),
            0xE0E0E0,
        ));
        match s.tally() {
            Some(t) => {
                l.push((format!("{:.1}% done · {} missing", t.percent(), t.missing), 0x55FF55));
                if t.wrong_block + t.wrong_state + t.extra > 0 {
                    l.push((
                        format!("{} wrong block · {} wrong state · {} extra", t.wrong_block, t.wrong_state, t.extra),
                        0xFFAA00,
                    ));
                }
            }
            None => l.push(("Verifying…".into(), 0xAAAAAA)),
        }
        if s.settings.printer_on {
            l.push((format!("Printer ON · {} placed", s.placed), 0xFF55FF));
            if !s.printer_note.is_empty() {
                l.push((s.printer_note.clone(), 0xAAAAAA));
            }
        }
        if s.settings.easy_place {
            l.push(("Easy place ON: only the schematic's blocks".to_string(), 0xFF55FF));
            if !s.easy_note.is_empty() {
                l.push((s.easy_note.clone(), 0xAAAAAA));
            }
        }
        if s.settings.highlight_missing || s.settings.highlight_wrong {
            let nearest = if s.mesh.marks_full { " (the nearest only)" } else { "" };
            l.push((format!("{} · {}{nearest}", highlight_text(s), s.mesh.marks_count), 0x55FFFF));
        }
        if let Some(d) = area {
            l.push((format!("Area: {} x {} x {}", d[0], d[1], d[2]), 0xE0E0E0));
        }
        Some(l)
    });
    let Some(mut lines) = lines else { return };
    if let Some((save, _)) = with(|s| save_lines(game, s)) {
        lines.extend(save);
    }
    lines.extend(with(|s| load_lines(s)));
    if tool && with(|s| s.settings.tool_hud) {
        lines.extend(with(|s| tool_lines(s)));
    }
    if !lines.is_empty() && game.native_corner_shown(Corner::TopLeft) {
        let w = lines.iter().map(|(t, _)| game.native_text_width(t)).fold(0.0f32, f32::max) + 8.0;
        let h = lines.len() as f32 * 10.0 + 6.0;
        let (x, y) = corners.place(Corner::TopLeft, w, h);
        game.native_fill(batch, [x, y, w, h], 0x9000_0000);
        for (i, (text, rgb)) in lines.iter().enumerate() {
            game.native_text(batch, text, [x + 4.0, y + 4.0 + i as f32 * 10.0], *rgb, true);
        }
    }
    if with(|s| s.settings.materials_hud) && game.native_corner_shown(Corner::TopRight) {
        let list = material_list(game);
        let rows: Vec<_> = list.iter().filter(|m| m.missing > 0).take(14).collect();
        let title = if rows.is_empty() { "Materials: nothing missing".to_string() } else { "Materials (missing / carried)".to_string() };
        let text = |m: &Material| format!("{}  {} / {}", short(&m.item), m.missing, m.available);
        let w = rows.iter().map(|m| game.native_text_width(&text(m)) + 16.0).fold(game.native_text_width(&title), f32::max) + 10.0;
        let h = 16.0 + rows.len() as f32 * 13.0;
        let (x, y) = corners.place(Corner::TopRight, w, h);
        game.native_fill(batch, [x, y, w, h], 0x9000_0000);
        game.native_text(batch, &title, [x + 4.0, y + 4.0], 0x55FFFF, true);
        for (i, m) in rows.iter().enumerate() {
            let ry = y + 15.0 + i as f32 * 13.0;
            game.native_item(batch, &m.item, [x + 4.0, ry], 12.0);
            let rgb = if m.to_fetch() == 0 { 0x55FF55 } else { 0xFF5555 };
            game.native_text(batch, &text(m), [x + 20.0, ry + 2.0], rgb, true);
        }
    }
}

// ---------------------------------------------------------------- the menu screen (M)

/// What the Load tab shows of a file (read when it is picked).
#[derive(Clone, Debug)]
struct FileInfo {
    name: String,
    format: String,
    size: [i32; 3],
    blocks: i64,
    regions: usize,
    author: String,
}

fn file_info(dir: &std::path::Path, name: &str) -> Result<FileInfo, String> {
    let bytes = std::fs::read(dir.join(name)).map_err(|e| e.to_string())?;
    let s = promatica_core::formats::summary(&bytes)?;
    Ok(FileInfo {
        name: if s.name.is_empty() { name.to_string() } else { s.name },
        format: s.format.name(),
        size: s.size,
        blocks: s.blocks as i64,
        regions: s.regions,
        author: s.author,
    })
}

/// What is known of a file; asked for the first time, a thread starts reading it (`None` until
/// it is in).
fn file_info_cached(name: &str, key: &str) -> Option<Result<FileInfo, String>> {
    let known = with(|s| {
        if let Some((_, rx)) = &s.ui_info_chan {
            while let Ok((k, info)) = rx.try_recv() {
                s.ui_info.insert(k, Some(info));
            }
        }
        s.ui_info.get(key).cloned()
    });
    if let Some(info) = known {
        return info;
    }
    let tx = with(|s| {
        s.ui_info.insert(key.to_string(), None);
        s.ui_info_chan.get_or_insert_with(std::sync::mpsc::channel).0.clone()
    });
    let (name, key) = (name.to_string(), key.to_string());
    let dir = schematics_dir();
    std::thread::spawn(move || {
        let info = dir.and_then(|d| file_info(&d, &name));
        let _ = tx.send((key, info));
    });
    None
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

/// `minecraft:oak_planks` as "Oak Planks".
fn pretty_item(item: &str) -> String {
    short(item)
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs a command for the screen; its first line (or the error) shows at the bottom.
fn act(game: &mut Game<'_>, args: &[&str]) {
    let msg = match run(game, args) {
        Ok(lines) => lines.into_iter().next().unwrap_or_default(),
        Err(e) => format!("Error: {e}"),
    };
    with(|s| s.ui_message = msg);
}

/// After a setting changed through the screen.
fn settings_changed(game: &Game<'_>) {
    with(|s| {
        s.settings_loaded = false;
        load_settings(game, s);
        s.mesh_key = None;
    });
}

/// The screen M opens, drawn with horizon-menus' widgets (the game's own buttons, sliders,
/// edit boxes and lists). The game keeps running behind it.
pub(super) fn screen(game: &mut Game<'_>, ui: &mut menus::Ui<'_>) {
    use winit::keyboard::KeyCode;
    with(|s| load_settings(game, s));
    let w = (ui.width - 20.0).min(500.0).floor();
    let x0 = ((ui.width - w) / 2.0).floor();
    let loaded = with(|s| {
        s.placement().map(|p| if s.slots.len() > 1 { format!("{} ({}/{})", display_name(p), s.selected + 1, s.slots.len()) } else { display_name(p) })
    });
    ui.text_centered(game, ui.width / 2.0, 7.0, "Promatica", 0xFFFFFF);
    if let Some(name) = &loaded {
        let shown = ui.fit(game, name, w / 2.0 - 50.0);
        ui.text_right(game, x0 + w, 7.0, &shown, 0x55FFFF);
    }
    // Eight fit with their names whole; the credits are a button of their own at the top.
    const TABS: [&str; 8] = ["Load", "Placement", "Save", "Tool", "Printer", "Display", "Materials", "Keys"];
    const CREDITS: usize = TABS.len();
    let tab = with(|s| s.ui_tab).min(CREDITS);
    if tab == 7 {
        // Before the keys below: the key being bound isn't also a shortcut.
        capture_key(game, ui);
    } else {
        with(|s| s.capture = None);
    }
    let mut next = ui.tabs(game, [x0, 20.0, w, 20.0], &TABS, tab);
    // 1-8 switch tabs, M closes (while no edit box has the keys).
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
    ];
    for (i, key) in digits
        .into_iter()
        .enumerate()
    {
        if ui.key(key) {
            next = Some(i);
        }
    }
    if ui.key(KeyCode::KeyM) {
        ui.close();
    }
    if let Some(t) = next {
        with(|s| s.ui_tab = t);
    }
    let body = [x0, 46.0, w, ui.height - 46.0 - 30.0];
    match tab {
        0 => tab_load(game, ui, body),
        1 => tab_placement(game, ui, body),
        2 => tab_save(game, ui, body),
        3 => tab_tool(game, ui, body),
        4 => tab_printer(game, ui, body),
        5 => tab_display(game, ui, body),
        6 => tab_materials(game, ui, body),
        7 => tab_keys(game, ui, body),
        _ => tab_credits(game, ui, body),
    }
    let fy = ui.height - 25.0;
    let msg = with(|s| s.ui_message.clone());
    if !msg.is_empty() {
        ui.text_fit(game, x0, fy + 6.0, w - 110.0, &msg, 0xFFFF55);
    }
    if ui.button(game, [x0, 2.0, 50.0, 16.0], "Credits", tab != CREDITS) {
        with(|s| s.ui_tab = CREDITS);
    }
    if ui.button(game, [x0 + w - 100.0, fy, 100.0, 20.0], "Done", true) {
        ui.close();
    }
}

fn tab_load(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    // The folder is listed again every few seconds while the tab is open.
    with(|s| {
        if s.ui_files_at.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(3)) {
            s.ui_files_at = Some(std::time::Instant::now());
            s.ui_files = schematics_dir()
                .map(|dir| {
                    promatica_core::list_schematics(&dir)
                        .into_iter()
                        .map(|n| {
                            let size = std::fs::metadata(dir.join(&n)).map_or(0, |m| m.len());
                            (n, size)
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
    });
    let lw = (r[2] * 0.55).floor();
    let query = ui.edit(game, "search", [r[0] + 1.0, r[1] + 1.0, lw - 2.0, 18.0], "Search...", 64).to_lowercase();
    let files: Vec<(String, u64)> =
        with(|s| s.ui_files.iter().filter(|(n, _)| n.to_lowercase().contains(&query)).cloned().collect());
    let total = with(|s| s.ui_files.len());
    let list = [r[0], r[1] + 24.0, lw, r[3] - 24.0];
    ui.fill(game, list, 0x9000_0000);
    ui.frame(game, list, 0xFF50_5050);
    let row_h = 14.0;
    let content = files.len() as f32 * row_h + 4.0;
    let (y0, mark) = ui.scroll_begin("files", list, content);
    let selected = with(|s| s.ui_file.clone());
    let mut picked = None;
    for (i, (name, size)) in files.iter().enumerate() {
        let y = y0 + 2.0 + i as f32 * row_h;
        if y + row_h < list[1] || y > list[1] + list[3] {
            continue;
        }
        let row = [list[0] + 2.0, y, list[2] - 10.0, row_h];
        let is_selected = selected.as_deref() == Some(name.as_str());
        if is_selected {
            ui.fill(game, row, 0x50FF_FFFF);
        } else if ui.hovered(row) {
            ui.fill(game, row, 0x25FF_FFFF);
        }
        let size = human_size(*size);
        let sw = ui.text_width(game, &size);
        ui.text_fit(game, row[0] + 3.0, y + 3.0, row[2] - sw - 12.0, name, if is_selected { 0xFFFFFF } else { 0xD0D0D0 });
        ui.text_right(game, row[0] + row[2] - 3.0, y + 3.0, &size, 0x909090);
        if ui.clicked(row) {
            picked = Some(name.clone());
        }
    }
    ui.scroll_end(game, "files", list, content, mark);
    if files.is_empty() {
        let text = if total == 0 { "No schematic files yet" } else { "Nothing matches" };
        ui.text(game, list[0] + 5.0, list[1] + 5.0, text, 0x909090);
    }
    if let Some(name) = picked {
        // A second click on the picked file loads it.
        if selected.as_deref() == Some(name.as_str()) {
            act(game, &["load", &name]);
            with(|s| s.ui_tab = 1);
            return;
        }
        with(|s| s.ui_file = Some(name));
    }

    // The picked file: what it is, and loading it.
    let rx = r[0] + lw + 8.0;
    let rw = r[2] - lw - 8.0;
    let selected = with(|s| s.ui_file.clone()).filter(|n| with(|s| s.ui_files.iter().any(|(f, _)| f == n)));
    let mut y = r[1] + 2.0;
    match &selected {
        Some(name) => {
            let size = with(|s| s.ui_files.iter().find(|(f, _)| f == name).map_or(0, |(_, b)| *b));
            let key = format!("{name}|{size}");
            match file_info_cached(name, &key) {
                None => {
                    ui.text_wrapped(game, rx, y, rw, "Reading the file...", 0xA0A0A0);
                }
                Some(Ok(i)) => {
                    y += ui.text_wrapped(game, rx, y, rw, &i.name, 0xFFFFFF) + 2.0;
                    let lines = [
                        (format!("{} x {} x {}", i.size[0], i.size[1], i.size[2]), 0xE0E0E0),
                        (format!("{} blocks", i.blocks), 0xE0E0E0),
                        (format!("{} region{}", i.regions, if i.regions == 1 { "" } else { "s" }), 0xA0A0A0),
                        (i.format.clone(), 0xA0A0A0),
                        (if i.author.is_empty() { String::new() } else { format!("by {}", i.author) }, 0xA0A0A0),
                    ];
                    for (text, rgb) in lines {
                        if !text.is_empty() {
                            ui.text_fit(game, rx, y, rw, &text, rgb);
                            y += 11.0;
                        }
                    }
                }
                Some(Err(e)) => {
                    ui.text_wrapped(game, rx, y, rw, &format!("Can't read it: {e}"), 0xFF5555);
                }
            }
            let by = r[1] + r[3] - 68.0;
            if ui.button_tip(game, [rx, by, rw, 20.0], "Load here", true, &["Places it at your feet (or click the file twice)"]) {
                act(game, &["load", name]);
                with(|s| s.ui_tab = 1);
            }
        }
        None => {
            ui.text_wrapped(game, rx, y, rw, "Pick a file on the left. Put .litematic, .schem, .schematic and .nbt files into the schematics folder.", 0xA0A0A0);
        }
    }
    let by = r[1] + r[3] - 44.0;
    let half = ((rw - 4.0) / 2.0).floor();
    if ui.button_tip(game, [rx, by, half, 20.0], "Reload last", true, &["Loads the last schematic again where it was"]) {
        act(game, &["reload"]);
    }
    if ui.button(game, [rx + half + 4.0, by, rw - half - 4.0, 20.0], "Open folder", true) {
        menu_action(game, "folder");
    }
    if ui.button(game, [rx, by + 24.0, rw, 20.0], "Refresh", true) {
        with(|s| {
            s.ui_files_at = None;
            s.ui_info.clear();
        });
    }
}

fn tab_placement(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    // Every placement: name, origin, hidden or locked.
    let rows: Vec<(String, [i32; 3], bool, bool)> =
        with(|s| s.slots.iter().map(|x| (display_name(&x.placement), x.placement.origin, x.shown, x.locked)).collect());
    if rows.is_empty() {
        ui.text(game, r[0], r[1] + 4.0, "No schematic loaded.", 0xA0A0A0);
        if ui.button(game, [r[0], r[1] + 18.0, 150.0, 20.0], "Load one", true) {
            with(|s| s.ui_tab = 0);
        }
        return;
    }
    let selected = with(|s| s.selected.min(s.slots.len() - 1));

    // Left: the list; a click selects.
    let lw = (r[2] * 0.38).floor();
    let list = [r[0], r[1], lw, r[3] - 96.0];
    ui.fill(game, list, 0x9000_0000);
    ui.frame(game, list, 0xFF50_5050);
    let row_h = 26.0;
    let content = rows.len() as f32 * row_h + 4.0;
    let (y0, mark) = ui.scroll_begin("placements", list, content);
    let mut picked = None;
    for (i, (name, origin, shown, locked)) in rows.iter().enumerate() {
        let y = y0 + 2.0 + i as f32 * row_h;
        if y + row_h < list[1] || y > list[1] + list[3] {
            continue;
        }
        let row = [list[0] + 2.0, y, list[2] - 10.0, row_h - 2.0];
        let is_selected = i == selected;
        if is_selected {
            ui.fill(game, row, 0x50FF_FFFF);
            ui.fill(game, [row[0], row[1], 2.0, row[3]], if *locked { 0xFFFF_AA00 } else { 0xFF55_FFFF });
        } else if ui.hovered(row) {
            ui.fill(game, row, 0x25FF_FFFF);
        }
        let rgb = match (is_selected, *shown) {
            (true, _) => 0xFFFFFF,
            (false, true) => 0xD0D0D0,
            (false, false) => 0x808080,
        };
        ui.text_fit(game, row[0] + 6.0, y + 3.0, row[2] - 10.0, &format!("{}. {name}", i + 1), rgb);
        let mut detail = format!("{} {} {}", origin[0], origin[1], origin[2]);
        if !shown {
            detail += " · hidden";
        }
        if *locked {
            detail += " · locked";
        }
        ui.text_fit(game, row[0] + 6.0, y + 13.0, row[2] - 10.0, &detail, if *locked { 0xFFAA00 } else { 0x909090 });
        if ui.clicked(row) {
            picked = Some(i);
        }
    }
    ui.scroll_end(game, "placements", list, content, mark);
    if let Some(i) = picked
        && i != selected
    {
        act(game, &["select", &(i + 1).to_string()]);
    }
    let by = r[1] + r[3] - 20.0;
    let event = {
        let mut store = menus::GameStore { game: &*game, id: ID };
        ui.settings(game, menu(), "easy_place", &mut store, [r[0], by - 72.0, lw, 0.0], 1).0
    };
    if event != menus::Event::None {
        settings_changed(game);
    }
    let half = ((lw - 4.0) / 2.0).floor();
    if ui.button_tip(game, [r[0], by, half, 20.0], "Load more", true, &["Another schematic, as one more placement"]) {
        with(|s| s.ui_tab = 0);
    }
    if ui.button_tip(game, [r[0] + half + 4.0, by, lw - half - 4.0, 20.0], "Unload all", true, &["Removes every placement"]) {
        act(game, &["unload", "all"]);
    }

    // Right: the selected placement.
    let info = with(|s| {
        let slot = s.sel()?;
        let p = &slot.placement;
        let (lo, hi) = p.bounds();
        Some((
            display_name(p),
            p.origin,
            [0, 1, 2].map(|i| hi[i] - lo[i] + 1),
            p.rotation.name(),
            p.mirror.name(),
            s.layer.describe(),
            s.moving,
            slot.shown,
            slot.locked,
            s.tally().map(|t| (t.percent(), t.missing, t.wrong_block + t.wrong_state, t.extra)),
            s.settings.tool_placement,
        ))
    });
    let Some((name, origin, size, rotation, mirror, layers, moving, shown, locked, tally, tool_placement)) = info else { return };
    let x = r[0] + lw + 8.0;
    // Right: the selected placement, scrolled where the window is too low for it (at 720p).
    let area = [x, r[1], r[2] - lw - 8.0, r[3]];
    let content = 37.0 + 8.0 * 24.0 - 4.0;
    let w = if content > area[3] { area[2] - 10.0 } else { area[2] };
    let free = !locked;
    let half = ((w - 4.0) / 2.0).floor();
    let third = ((w - 8.0) / 3.0).floor();
    let (mut y, mark) = ui.scroll_begin("placement_info", area, content);
    ui.text_fit(game, x, y + 1.0, w, &format!("{}. {name}", selected + 1), 0xFFFFFF);
    y += 12.0;
    ui.text_fit(game, x, y, w, &format!("{} x {} x {} · rotation {rotation} · mirror {mirror}", size[0], size[1], size[2]), 0xC0C0C0);
    y += 11.0;
    match tally {
        Some((pct, missing, wrong, extra)) => {
            ui.text_fit(game, x, y, w, &format!("{pct:.1}% done · {missing} missing · {wrong} wrong · {extra} extra"), 0x55FF55)
        }
        None => ui.text_fit(game, x, y, w, "Checking the world...", 0xA0A0A0),
    }
    y += 14.0;

    // Shown, locked, gone.
    let tip = ["Hidden placements stay loaded but aren't drawn or printed"];
    if ui.button_tip(game, [x, y, third, 20.0], if shown { "Hide" } else { "Show" }, true, &tip) {
        act(game, &["show", "toggle"]);
    }
    let tip = ["Locked: it can't be moved, turned or mirrored", "(neither here, nor by commands or the tool)"];
    if ui.button_tip(game, [x + third + 4.0, y, third, 20.0], if locked { "Unlock" } else { "Lock" }, true, &tip) {
        act(game, &["lock", "toggle"]);
    }
    if ui.button_tip(game, [x + 2.0 * (third + 4.0), y, w - 2.0 * (third + 4.0), 20.0], "Unload", true, &["Removes this placement (the world stays as it is)"]) {
        act(game, &["unload"]);
    }
    y += 24.0;

    // Where it is.
    ui.text(game, x, y + 6.0, "Origin", if free { 0xFFFFFF } else { 0x808080 });
    // The fields follow the origin unless one is being typed in.
    let typing = ui.focused("ox") || ui.focused("oy") || ui.focused("oz");
    if !typing && with(|s| s.ui_origin != Some(origin)) {
        ui.set_edit("ox", &origin[0].to_string());
        ui.set_edit("oy", &origin[1].to_string());
        ui.set_edit("oz", &origin[2].to_string());
        with(|s| s.ui_origin = Some(origin));
    }
    let fx = x + 36.0;
    let fw = ((w - 36.0 - 40.0 - 6.0) / 3.0).floor();
    let ox = ui.edit(game, "ox", [fx, y + 1.0, fw - 2.0, 18.0], "x", 9);
    let oy = ui.edit(game, "oy", [fx + fw, y + 1.0, fw - 2.0, 18.0], "y", 9);
    let oz = ui.edit(game, "oz", [fx + 2.0 * fw, y + 1.0, fw - 2.0, 18.0], "z", 9);
    let set = ui.button(game, [fx + 3.0 * fw + 2.0, y, 40.0, 20.0], "Set", free) || (typing && ui.key(winit::keyboard::KeyCode::Enter));
    if set {
        match (ox.trim().parse::<i32>(), oy.trim().parse::<i32>(), oz.trim().parse::<i32>()) {
            (Ok(a), Ok(b), Ok(c)) => act(game, &["origin", &a.to_string(), &b.to_string(), &c.to_string()]),
            _ => with(|s| s.ui_message = "Origin: three whole numbers".into()),
        }
        with(|s| s.ui_origin = None);
    }
    y += 24.0;
    // Nudge buttons: 1 block, 10 with Shift.
    let step = if ui.shift { 10 } else { 1 };
    let bw = ((w - 10.0) / 6.0).floor();
    let nudges: [(&str, [i32; 3]); 6] =
        [("-X", [-1, 0, 0]), ("+X", [1, 0, 0]), ("-Y", [0, -1, 0]), ("+Y", [0, 1, 0]), ("-Z", [0, 0, -1]), ("+Z", [0, 0, 1])];
    for (i, (label, d)) in nudges.into_iter().enumerate() {
        let tip = format!("Move {step} block{} ({label}); Shift: 10", if step == 1 { "" } else { "s" });
        if ui.button_tip(game, [x + i as f32 * (bw + 2.0), y, bw, 20.0], label, free, &[&tip]) {
            let d = d.map(|v| (v * step).to_string());
            act(game, &["move", &d[0], &d[1], &d[2]]);
        }
    }
    y += 24.0;
    if ui.button(game, [x, y, half, 20.0], "Move to me", free) {
        act(game, &["origin", "here"]);
    }
    let label = format!("Move mode: {}", if moving { "ON" } else { "OFF" });
    if ui.button_tip(game, [x + half + 4.0, y, w - half - 4.0, 20.0], &label, free, &["Wheel, arrows and PgUp/PgDn move it in the world (N)"]) {
        toggle_move_mode(game);
        if with(|s| s.moving) {
            ui.close();
        }
    }
    y += 24.0;

    // How it is turned.
    if ui.button(game, [x, y, third, 20.0], "Rotate left", free) {
        act(game, &["rotate", "ccw"]);
    }
    if ui.button(game, [x + third + 4.0, y, third, 20.0], "Rotate right", free) {
        act(game, &["rotate", "cw"]);
    }
    let tip = ["none, x (front-back), z (left-right)"];
    if ui.button_tip(game, [x + 2.0 * (third + 4.0), y, w - 2.0 * (third + 4.0), 20.0], &format!("Mirror: {mirror}"), free, &tip) {
        menu_action(game, "mirror_next");
    }
    y += 24.0;

    // Layers (of every placement).
    if ui.button(game, [x, y, w, 20.0], &format!("Layers: {layers}"), true) {
        let next = next_layer_mode();
        act(game, &["layer", next]);
    }
    y += 24.0;
    if ui.button(game, [x, y, third, 20.0], "Layer down", true) {
        act(game, &["layer", "down"]);
    }
    if ui.button(game, [x + third + 4.0, y, third, 20.0], "Layer up", true) {
        act(game, &["layer", "up"]);
    }
    if ui.button(game, [x + 2.0 * (third + 4.0), y, w - 2.0 * (third + 4.0), 20.0], "All layers", true) {
        act(game, &["layer", "all"]);
    }
    y += 24.0;

    // The tool moves placements in its placement mode.
    let label = format!("Selection tool: {} mode", if tool_placement { "placement" } else { "area" });
    let tip = [
        "Placement mode: left click selects the placement looked at,",
        "right click moves the selected one there, Alt+wheel moves it,",
        "Ctrl+wheel turns it. Ctrl+middle click switches the mode.",
    ];
    if ui.button_tip(game, [x, y, w, 20.0], &label, true, &tip) {
        act(game, &["tool", "mode", "toggle"]);
    }
    ui.scroll_end(game, "placement_info", area, content, mark);
}

fn tab_save(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let area = with(|s| s.area);
    let mut y = r[1];
    ui.text(game, r[0], y + 2.0, "Area selection: two opposite corners of the box to save", 0xFFFFFF);
    y += 16.0;
    let label_w = 52.0;
    let bw = 48.0;
    let fw = ((r[2] - label_w - 40.0 - 2.0 * bw - 3.0 * 4.0) / 3.0).floor();
    for (i, corner) in area.into_iter().enumerate() {
        let ids = [0, 1, 2].map(|k| format!("c{}{}", i + 1, ["x", "y", "z"][k]));
        // The fields follow the corner unless one is being typed in.
        let typing = ids.iter().any(|id| ui.focused(id));
        if !typing && with(|s| s.ui_corners[i] != corner) {
            for (k, id) in ids.iter().enumerate() {
                ui.set_edit(id, &corner.map_or(String::new(), |c| c[k].to_string()));
            }
            with(|s| s.ui_corners[i] = corner);
        }
        let rgb = if i == 0 { 0xFF7070 } else { 0x7090FF };
        ui.text(game, r[0], y + 6.0, &format!("Corner {}", i + 1), rgb);
        let fx = r[0] + label_w;
        let v = [0, 1, 2].map(|k| ui.edit(game, &ids[k], [fx + k as f32 * fw, y + 1.0, fw - 2.0, 18.0], ["x", "y", "z"][k], 9));
        let mut x = fx + 3.0 * fw + 2.0;
        let cmd = if i == 0 { "pos1" } else { "pos2" };
        if ui.button(game, [x, y, 40.0, 20.0], "Set", true) {
            match v.iter().map(|s| s.trim().parse::<i32>()).collect::<Result<Vec<_>, _>>() {
                Ok(c) => act(game, &[cmd, &c[0].to_string(), &c[1].to_string(), &c[2].to_string()]),
                Err(_) => with(|s| s.ui_message = format!("Corner {}: three whole numbers", i + 1)),
            }
            with(|s| s.ui_corners[i] = None);
        }
        x += 44.0;
        if ui.button_tip(game, [x, y, bw, 20.0], "Look", true, &["The block you look at (/hm pos1, /hm pos2)"]) {
            act(game, &[cmd]);
        }
        x += bw + 4.0;
        if ui.button_tip(game, [x, y, bw, 20.0], "Here", true, &["The block you stand in"]) {
            act(game, &[cmd, "here"]);
        }
        y += 24.0;
    }
    let size = with(|s| s.area_box().map(|(lo, hi)| [0, 1, 2].map(|k| hi[k] - lo[k] + 1)));
    let text = match size {
        Some(d) => format!("{} x {} x {} = {} blocks", d[0], d[1], d[2], d.iter().map(|&v| i64::from(v)).product::<i64>()),
        None => "Set both corners.".to_string(),
    };
    ui.text(game, r[0], y + 2.0, &text, if size.is_some() { 0x55FF55 } else { 0xA0A0A0 });
    y += 16.0;
    ui.text(game, r[0], y + 6.0, "Name", 0xFFFFFF);
    let name = ui.edit(game, "save_name", [r[0] + label_w, y + 1.0, r[2] - label_w - 104.0, 18.0], "my_build", 80);
    let stem = file_stem(&name);
    let exists = stem.as_ref().is_ok_and(|s| schematics_dir().is_ok_and(|d| d.join(format!("{s}.litematic")).exists()));
    let confirm = with(|s| s.ui_overwrite.clone()).is_some_and(|n| stem.as_ref().is_ok_and(|s| *s == n));
    let label = if exists && confirm { "Overwrite" } else { "Save" };
    let tip = ["Saves the area as a .litematic into the schematics folder", "(chunk by chunk, as they are loaded)"];
    let progress = with(|s| save_lines(game, s));
    if let Some((lines, done)) = &progress {
        // While it runs: Cancel in Save's place, the progress above the note at the bottom.
        if ui.button_tip(game, [r[0] + r[2] - 100.0, y, 100.0, 20.0], "Cancel", with(|s| s.save.is_some()), &["Stops the save; nothing is written"]) {
            act(game, &["save", "cancel"]);
        }
        let bar = [r[0], r[1] + r[3] - 80.0, r[2], 20.0];
        ui.fill(game, bar, 0xC000_0000);
        ui.fill(game, [bar[0], bar[1], (bar[2] * done).floor(), bar[3]], 0xC0FF_8800);
        ui.frame(game, bar, 0xFF80_8080);
        ui.text_fit(game, bar[0] + 4.0, bar[1] + 6.0, bar[2] - 8.0, &lines[0].0, 0xFFFFFF);
        if let Some((text, rgb)) = lines.get(1) {
            ui.text_fit(game, r[0], bar[1] + 24.0, r[2], text, *rgb);
        }
    } else if ui.button_tip(game, [r[0] + r[2] - 100.0, y, 100.0, 20.0], label, size.is_some() && stem.is_ok(), &tip) {
        match &stem {
            Ok(s) if exists && !confirm => with(|st| {
                st.ui_overwrite = Some(s.clone());
                st.ui_message = format!("{s}.litematic exists: click Overwrite to replace it");
            }),
            _ => {
                let msg = match start_save(game, &name, exists) {
                    Ok(m) => m,
                    Err(e) => format!("Error: {e}"),
                };
                with(|s| {
                    s.ui_message = msg;
                    s.ui_overwrite = None;
                });
            }
        }
    }
    y += 26.0;
    let half = ((r[2] - 4.0) / 2.0).floor();
    if ui.button(game, [r[0], y, half, 20.0], "Clear selection", area.iter().any(Option::is_some)) {
        act(game, &["area", "clear"]);
    }
    if ui.button(game, [r[0] + half + 4.0, y, r[2] - half - 4.0, 20.0], "Open folder", true) {
        menu_action(game, "folder");
    }
    y += 26.0;
    let (tool, item) = with(|s| (s.settings.tool, s.settings.tool_item.clone()));
    let label = format!("Selection tool: {} ({}) · settings", if tool { "ON" } else { "OFF" }, short(&item));
    let tip = ["Opens the Tool tab: which item, its reach, its controls"];
    if ui.button_tip(game, [r[0], y, r[2], 20.0], &label, true, &tip) {
        with(|s| s.ui_tab = 3);
    }
    if tool {
        ui.text_wrapped(
            game,
            r[0],
            y + 26.0,
            r[2],
            &format!("With the {} in hand: left click corner 1, right click corner 2.", short(&item).replace('_', " ")),
            0xA0A0A0,
        );
    }
    ui.text_wrapped(
        game,
        r[0],
        r[1] + r[3] - 30.0,
        r[2],
        "Saves the blocks with their states, as Litematica reads them. Not saved: what is inside chests, sign texts and entities.",
        0x808080,
    );
}

/// Items the Tool tab offers at a click.
const TOOL_ITEMS: [&str; 8] = [
    "minecraft:stick",
    "minecraft:blaze_rod",
    "minecraft:breeze_rod",
    "minecraft:bone",
    "minecraft:feather",
    "minecraft:wooden_axe",
    "minecraft:golden_hoe",
    "minecraft:arrow",
];

/// The selection tool: which item it is, how it aims, and its controls.
fn tab_tool(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let (on, item) = with(|s| (s.settings.tool, s.settings.tool_item.clone()));
    let mut y = r[1];
    // The tool, large, and whether it is in hand.
    ui.fill(game, [r[0], y, 40.0, 40.0], 0x9000_0000);
    ui.frame(game, [r[0], y, 40.0, 40.0], if on { 0xFF55_FF55 } else { 0xFF80_8080 });
    ui.item(game, r[0] + 4.0, y + 4.0, &item, 32.0);
    let held = game.inventory.get(game.selected).and_then(Option::as_ref).map(|st| st.item.to_string());
    ui.text(game, r[0] + 48.0, y + 6.0, &format!("Selection tool: {}", short(&item)), 0xFFFFFF);
    let (state, rgb) = match (on, held.as_deref() == Some(item.as_str())) {
        (false, _) => ("Off: clicks with it break and use as usual".to_string(), 0xFF7070),
        (true, true) => ("On, and in your hand: click blocks to set the corners".to_string(), 0x55FF55),
        (true, false) => ("On: take it in your main hand to select".to_string(), 0xFFFF55),
    };
    ui.text(game, r[0] + 48.0, y + 22.0, &state, rgb);
    y += 48.0;
    ui.text(game, r[0], y + 2.0, "Item: pick one, type its id, or take the one in your hand", 0xFFFFFF);
    y += 14.0;
    let size = 22.0;
    for (i, it) in TOOL_ITEMS.iter().enumerate() {
        let x = r[0] + i as f32 * (size + 3.0);
        let current = *it == item;
        if ui.button_tip(game, [x, y, size, size], "", !current, &[short(it)]) {
            act(game, &["tool", *it]);
        }
        ui.item(game, x + 3.0, y + 3.0, it, 16.0);
        if current {
            ui.frame(game, [x - 1.0, y - 1.0, size + 2.0, size + 2.0], 0xFFFF_FF55);
        }
    }
    let ex = r[0] + TOOL_ITEMS.len() as f32 * (size + 3.0) + 4.0;
    let (set_w, held_w) = (36.0, 72.0);
    let ew = r[0] + r[2] - ex - set_w - held_w - 8.0;
    let typed = ui.edit(game, "tool_item", [ex, y + 2.0, ew, 18.0], "item id", 64);
    if ui.button(game, [ex + ew + 4.0, y + 1.0, set_w, 20.0], "Set", !typed.trim().is_empty()) {
        act(game, &["tool", typed.trim()]);
        ui.set_edit("tool_item", "");
    }
    let tip = ["The item in your main hand becomes the tool"];
    if ui.button_tip(game, [r[0] + r[2] - held_w, y + 1.0, held_w, 20.0], "Held item", held.is_some(), &tip)
        && let Some(h) = &held
    {
        act(game, &["tool", h.as_str()]);
    }
    y += size + 10.0;
    let (event, _) = {
        let mut store = menus::GameStore { game: &*game, id: ID };
        ui.settings(game, menu(), "selection", &mut store, [r[0], y, r[2], 0.0], 2)
    };
    match event {
        menus::Event::None => {}
        menus::Event::Changed(_) => settings_changed(game),
        menus::Event::Action(a) => menu_action(game, &a),
    }
}

/// The Keys tab: every action with its key; a click on the key waits for a new one.
fn tab_keys(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let (binds, capture) = with(|s| (s.binds.clone().unwrap_or_default(), s.capture.clone()));
    let list = [r[0], r[1], r[2], r[3] - 24.0];
    ui.fill(game, list, 0x9000_0000);
    ui.frame(game, list, 0xFF50_5050);
    let row_h = 22.0;
    let content = BINDS.len() as f32 * row_h + 4.0;
    let (y0, mark) = ui.scroll_begin("keys", list, content);
    let button_w = 110.0;
    let mut picked = None;
    for (i, (id, label, _)) in BINDS.iter().enumerate() {
        let y = y0 + 2.0 + i as f32 * row_h;
        if y + row_h < list[1] || y > list[1] + list[3] {
            continue;
        }
        let key = binds.iter().find(|(b, _)| b == id).map_or("", |(_, k)| k.as_str());
        let waiting = capture.as_deref() == Some(*id);
        ui.text_fit(game, list[0] + 6.0, y + 6.0, list[2] - button_w - 24.0, label, 0xE0E0E0);
        let shown = if waiting { "> press a key <".to_string() } else { key_label(key) };
        if ui.button(game, [list[0] + list[2] - button_w - 10.0, y + 1.0, button_w, 20.0], &shown, true) {
            picked = Some(if waiting { None } else { Some(id.to_string()) });
        }
    }
    ui.scroll_end(game, "keys", list, content, mark);
    if let Some(next) = picked {
        with(|s| s.capture = next);
    }
    let by = r[1] + r[3] - 20.0;
    if ui.button(game, [r[0], by, 120.0, 20.0], "Reset all keys", true) {
        reset_binds(game);
        with(|s| {
            s.capture = None;
            s.ui_message = "All keys are back to their defaults".into();
        });
    }
    let hint = if capture.is_some() { "Press a key. Backspace clears, a click cancels." } else { "Click a key to change it." };
    ui.text_fit(game, r[0] + 128.0, by + 6.0, r[2] - 128.0, hint, 0xA0A0A0);
}

fn tab_printer(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let (on, placed, note, loaded) =
        with(|s| (s.settings.printer_on, s.placed, s.printer_note.clone(), !s.slots.is_empty()));
    let label = if on { "Printer: ON (building)" } else { "Printer: OFF" };
    if ui.button_tip(game, [r[0], r[1], r[2], 20.0], label, loaded, &["Places missing blocks within reach (K)"]) {
        act(game, &["printer"]);
    }
    let mut y = r[1] + 26.0;
    if !loaded {
        ui.text(game, r[0], y, "Load a schematic first.", 0xA0A0A0);
        y += 12.0;
    }
    ui.text(game, r[0], y, &format!("Placed this session: {placed}"), 0xE0E0E0);
    y += 11.0;
    if !note.is_empty() {
        ui.text_fit(game, r[0], y, r[2], &note, 0xA0A0A0);
    }
    y += 16.0;
    let event = {
        let mut store = menus::GameStore { game: &*game, id: ID };
        ui.settings(game, menu(), "printer", &mut store, [r[0], y, r[2], 0.0], 2).0
    };
    if event != menus::Event::None {
        settings_changed(game);
    }
    ui.text_wrapped(
        game,
        r[0],
        r[1] + r[3] - 20.0,
        r[2],
        "Survival: the blocks must be in your hotbar. On servers, automatic placing may break rules or trip anti-cheat.",
        0x808080,
    );
}

fn tab_display(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let mut events = Vec::new();
    let (faces, sections, todo, radius, limit) =
        with(|s| (s.mesh.quads, s.mesh.built.len(), s.mesh.todo, render_radius(game, &s.settings), s.settings.max_faces));
    let full = limit != 0 && faces >= limit;
    let line = format!(
        "Overlay: {faces} faces in {sections} sections, ~{} video memory · radius {radius} blocks{}",
        mib(faces * FACE_BYTES),
        if full { " · FULL".to_string() } else if todo > 0 { format!(" · {todo} to build") } else { String::new() }
    );
    ui.text_fit(game, r[0], r[1] + r[3] - 22.0, r[2], &line, if full { 0xFFFF55 } else { 0xA0A0A0 });
    ui.text_fit(game, r[0], r[1] + r[3] - 11.0, r[2], "Beyond the menu: /hm radius <blocks>, /hm faces <n>|none", 0x808080);
    let mut y = r[1];
    for (title, page) in [("World overlay", "display"), ("HUD", "hud")] {
        ui.text(game, r[0], y + 2.0, title, 0xFFFFFF);
        y += 13.0;
        let (event, used) = {
            let mut store = menus::GameStore { game: &*game, id: ID };
            ui.settings(game, menu(), page, &mut store, [r[0], y, r[2], 0.0], 2)
        };
        events.push(event);
        y += used + 4.0;
    }
    for event in events {
        match event {
            menus::Event::None => {}
            menus::Event::Changed(_) => settings_changed(game),
            menus::Event::Action(a) => menu_action(game, &a),
        }
    }
}

fn tab_materials(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let list = material_list(game);
    let hud = with(|s| s.settings.materials_hud);
    let bw = 150.0;
    if ui.button_tip(game, [r[0] + r[2] - bw, r[1], bw, 20.0], &format!("HUD list: {}", if hud { "ON" } else { "OFF" }), true, &["The list in the corner while playing (J)"]) {
        with(|s| {
            s.settings.materials_hud = !s.settings.materials_hud;
            save_settings(game, &s.settings);
        });
    }
    let tip = ["Copies the list as a table: paste it into a spreadsheet"];
    if ui.button_tip(game, [r[0] + r[2] - bw - 74.0, r[1], 70.0, 20.0], "Copy", !list.is_empty(), &tip) {
        act(game, &["materials", "copy"]);
    }
    let fetch: u64 = list.iter().map(|m| m.to_fetch()).sum();
    let summary = if list.is_empty() {
        "No schematic loaded, or it needs no items.".to_string()
    } else {
        format!("{} kinds, {} items still to fetch", list.len(), fetch)
    };
    ui.text(game, r[0], r[1] + 6.0, &summary, 0xE0E0E0);
    // The highlights: what they mark, and a row of the list below picks one item.
    let (missing, wrong, only) = with(|s| (s.settings.highlight_missing, s.settings.highlight_wrong, s.highlight_item.clone()));
    let on_off_text = |on: bool| if on { "ON" } else { "OFF" };
    ui.text(game, r[0], r[1] + 30.0, "Highlight", 0xE0E0E0);
    let toggle = |game: &mut Game<'_>, change: &dyn Fn(&mut Settings)| {
        with(|s| {
            change(&mut s.settings);
            save_settings(game, &s.settings);
        });
    };
    let tip = ["Marks them through everything, however far"];
    if ui.button_tip(game, [r[0] + 52.0, r[1] + 24.0, 86.0, 20.0], &format!("Missing: {}", on_off_text(missing)), true, &tip) {
        toggle(game, &|st: &mut Settings| st.highlight_missing = !st.highlight_missing);
    }
    if ui.button_tip(game, [r[0] + 142.0, r[1] + 24.0, 80.0, 20.0], &format!("Wrong: {}", on_off_text(wrong)), true, &tip) {
        toggle(game, &|st: &mut Settings| st.highlight_wrong = !st.highlight_wrong);
    }
    let shown = only.as_deref().map_or_else(|| "All items".to_string(), pretty_item);
    let tip = ["Click a row below to mark only that item, again to mark all"];
    if ui.button_tip(game, [r[0] + 226.0, r[1] + 24.0, r[2] - 226.0, 20.0], &shown, only.is_some(), &tip) {
        with(|s| s.highlight_item = None);
    }
    let table = [r[0], r[1] + 48.0, r[2], r[3] - 48.0];
    let cols = [table[0] + table[2] - 150.0, table[0] + table[2] - 90.0, table[0] + table[2] - 12.0];
    ui.text(game, table[0] + 24.0, table[1], "Item", 0xFFFFFF);
    ui.text_right(game, cols[0], table[1], "Missing", 0xFFFFFF);
    ui.text_right(game, cols[1], table[1], "Total", 0xFFFFFF);
    ui.text_right(game, cols[2], table[1], "Carried", 0xFFFFFF);
    let area = [table[0], table[1] + 11.0, table[2], table[3] - 11.0];
    ui.fill(game, area, 0x9000_0000);
    let row_h = 18.0;
    let content = list.len() as f32 * row_h + 2.0;
    let (y0, mark) = ui.scroll_begin("materials", area, content);
    for (i, m) in list.iter().enumerate() {
        let y = y0 + 1.0 + i as f32 * row_h;
        if y + row_h < area[1] || y > area[1] + area[3] {
            continue;
        }
        let row = [area[0] + 1.0, y, area[2] - 8.0, row_h];
        if i % 2 == 1 {
            ui.fill(game, row, 0x18FF_FFFF);
        }
        let name = pretty_item(&m.item);
        if only.as_deref() == Some(m.item.as_str()) {
            ui.fill(game, row, 0x5055_FFFF);
        }
        if ui.hovered(row) && ui.hovered(area) {
            ui.fill(game, row, 0x25FF_FFFF);
            ui.tooltip(&[&name, &format!("{} still to fetch", m.to_fetch()), "Click: highlight only this", &m.item]);
        }
        if ui.clicked(row) && ui.hovered(area) {
            with(|s| {
                let again = s.highlight_item.as_deref() == Some(m.item.as_str());
                s.highlight_item = (!again).then(|| m.item.clone());
                if !again && !s.settings.highlight_wrong {
                    s.settings.highlight_missing = true;
                }
                save_settings(game, &s.settings);
            });
        }
        ui.item(game, row[0] + 2.0, y + 1.0, &m.item, 16.0);
        ui.text_fit(game, row[0] + 23.0, y + 5.0, cols[0] - row[0] - 80.0, &name, 0xE0E0E0);
        let done = m.to_fetch() == 0;
        ui.text_right(game, cols[0], y + 5.0, &m.missing.to_string(), if m.missing == 0 { 0x55FF55 } else { 0xFF7070 });
        ui.text_right(game, cols[1], y + 5.0, &m.total.to_string(), 0xA0A0A0);
        ui.text_right(game, cols[2], y + 5.0, &m.available.to_string(), if done { 0x55FF55 } else { 0xFFAA00 });
    }
    ui.scroll_end(game, "materials", area, content, mark);
}

fn tab_credits(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let cx = r[0] + r[2] / 2.0;
    let mut y = r[1] + 4.0;
    ui.text_centered(game, cx, y, "Promatica re-creates Litematica for Horizon.", 0xFFFFFF);
    y += 12.0;
    ui.text_centered(game, cx, y, "All credit for the idea and design goes to:", 0xA0A0A0);
    y += 20.0;
    let people: [(&str, &str, &str); 4] = [
        ("Litematica", "by masa (maruohon), maintained by Sakura-Ryoko", "github.com/maruohon/litematica"),
        ("Litematica Printer", "by aleksilassila, maintained by Sakura-Ryoko", "github.com/aleksilassila/litematica-printer"),
        ("Horizon", "by the horizon-rust team", "github.com/horizon-rust/horizon"),
        ("File formats", "Litematica, Sponge/WorldEdit, MCEdit, Mojang", ".litematic .schem .schematic .nbt"),
    ];
    for (what, who, link) in people {
        ui.text_centered(game, cx, y, what, 0x55FFFF);
        y += 11.0;
        ui.text_centered(game, cx, y, who, 0xE0E0E0);
        y += 11.0;
        ui.text_centered(game, cx, y, link, 0x808080);
        y += 17.0;
    }
    ui.text_centered(game, cx, y, "Not made or endorsed by them. No code copied: format and behaviour re-created.", 0xA0A0A0);
    if ui.button(game, [cx - 100.0, r[1] + r[3] - 20.0, 200.0, 20.0], "Show the credits in chat", true) {
        menu_action(game, "credits_chat");
    }
}
