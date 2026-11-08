// horizon-menus, game side: a module a mod inserts into `crates/horizon-client/src/game.rs`
// (`[[module]]`, name `<mod id>_menus`), next to its own crate copy of `horizon-menus`
// (`[[crate]] id = "menus"`). tools/vendor_menus.py copies both into a bundle and names the
// crate `<mod id>_menus`; `horizon_menus` below is replaced with that name.
//
// Two ways to show a mod's menu:
//
// - **The mod's page** in Options > Mods (`GameStore`, `open_page`): buttons only, official
//   hooks only.
// - **A screen of its own** (`open_screen`, `draw`, `Ui`): drawn with the game's own widgets
//   (buttons, sliders, edit boxes, scroll lists, tooltips, item icons). It takes the place of the
//   Debug Options screen (F3+F6) while open: that screen's draw and input functions are hooked
//   (hooks/*.rs) and do nothing of their own while the game hasn't opened it (`debug_options` is
//   None). The game doesn't pause for it, as a mod's menu in Java doesn't.

use super::{Game, Screen};
use horizon_client::hud::{HudBatch, rgba};
use horizon_client::text::Text;
use horizon_client::ui::{self, EditBox};
// Re-exported for the mod; a mod uses some of them.
#[allow(unused_imports)]
pub(crate) use horizon_menus::{Control, Entry, Event, Menu, Nav, Setting, Store};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::ControlFlow;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

// ---------------------------------------------------------------- settings store, mod page

/// Horizon's settings of mod `id` (`native_setting` / `native_set_setting`, saved by the game).
pub(super) struct GameStore<'a, 'g> {
    pub(super) game: &'a Game<'g>,
    pub(super) id: &'a str,
}

impl Store for GameStore<'_, '_> {
    fn get(&self, key: &str) -> Option<String> {
        self.game.native_setting(self.id, key)
    }
    fn set(&mut self, key: &str, value: &str) {
        self.game.native_set_setting(self.id, key, value);
    }
}

/// Shift held: clicks step numbers and choices backwards.
#[allow(dead_code)]
pub(super) fn reverse(game: &Game<'_>) -> bool {
    game.keys.contains(&KeyCode::ShiftLeft) || game.keys.contains(&KeyCode::ShiftRight)
}

/// Opens mod `id`'s page in Options > Mods over the game, as Horizon's own tweaks key opens its
/// page: Esc goes straight back to playing. False when there is no such page (not in a world,
/// or the mod isn't listed).
#[allow(dead_code)]
pub(super) fn open_page(game: &mut Game<'_>, id: &str) -> bool {
    if game.title.is_some() {
        return false;
    }
    let find = |game: &Game<'_>| game.settings.mods.installed.iter().position(|m| m.id == id && m.native.is_some());
    if find(game).is_none() {
        game.list_native_mods();
    }
    let Some(i) = find(game) else { return false };
    game.capture(false);
    game.open_options_at(false, horizon_client::options_ui::Page::NativeMod(i));
    game.options_to_game = true;
    game.native_settings_refresh(i);
    true
}

// ---------------------------------------------------------------- the screen: state and hooks

/// What outlives a frame: whether the screen is open, the input since the last frame, and the
/// widgets' state (scroll offsets, edit boxes, the held slider).
#[derive(Default)]
struct State {
    open: bool,
    presses: Vec<(f32, f32)>,
    mouse_down: bool,
    released: bool,
    wheel: f32,
    keys: Vec<KeyCode>,
    held: Option<String>,
    scroll: HashMap<String, f32>,
    edits: HashMap<String, EditBox>,
    focus: Option<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Whether this mod's screen is open (it is closed when the game moved to another screen).
#[allow(dead_code)]
pub(super) fn is_open(game: &Game<'_>) -> bool {
    st(|s| {
        if s.open && game.screen != Screen::DebugOptions {
            s.open = false;
        }
        s.open
    })
}

/// Opens the mod's screen over the game. False when a screen is already open or there is no
/// world.
#[allow(dead_code)]
pub(super) fn open_screen(game: &mut Game<'_>) -> bool {
    if game.title.is_some() || game.screen != Screen::Playing {
        return false;
    }
    game.debug_options = None;
    game.screen = Screen::DebugOptions;
    game.capture(false);
    game.mining = None;
    game.keys.clear();
    st(|s| {
        s.open = true;
        s.presses.clear();
        s.mouse_down = false;
        s.released = false;
        s.wheel = 0.0;
        s.keys.clear();
        s.held = None;
        s.focus = None;
    });
    true
}

/// Closes the screen: back to playing.
#[allow(dead_code)]
pub(super) fn close_screen(game: &mut Game<'_>) {
    if is_open(game) {
        // The game's own close: playing, the mouse captured again (and `hook_close`).
        game.close_debug_options();
    }
    st(|s| s.open = false);
}

/// `close_debug_options` (Esc, F3+F6): the screen is gone.
#[allow(dead_code)]
pub(super) fn hook_close(_game: &mut Game<'_>) {
    st(|s| {
        s.open = false;
        s.focus = None;
        s.held = None;
    });
}

/// A left press on the screen, at the pointer.
#[allow(dead_code)]
pub(super) fn hook_click(game: &mut Game<'_>) {
    if is_open(game) {
        let at = game.cursor;
        st(|s| {
            s.presses.push(at);
            s.mouse_down = true;
        });
    }
}

#[allow(dead_code)]
pub(super) fn hook_release(game: &mut Game<'_>) {
    if is_open(game) {
        st(|s| {
            s.mouse_down = false;
            s.released = true;
        });
    }
}

#[allow(dead_code)]
pub(super) fn hook_scroll(game: &mut Game<'_>, delta: f32) {
    if is_open(game) {
        st(|s| s.wheel += delta);
    }
}

/// A key on the screen: the focused edit box's, Esc (leaves the box, then the screen), or the
/// mod's (`Ui::key`). The game's own handling of the key is skipped.
#[allow(dead_code)]
pub(super) fn hook_key(game: &mut Game<'_>, event: &KeyEvent) -> ControlFlow<()> {
    if !is_open(game) {
        return ControlFlow::Continue(());
    }
    if event.state != ElementState::Pressed {
        return ControlFlow::Break(());
    }
    let PhysicalKey::Code(key) = event.physical_key else { return ControlFlow::Break(()) };
    let focus = st(|s| s.focus.clone());
    if key == KeyCode::Escape {
        if focus.is_some() {
            st(|s| s.focus = None);
        } else {
            close_screen(game);
        }
        return ControlFlow::Break(());
    }
    if let Some(id) = focus {
        if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Tab) {
            st(|s| {
                s.focus = None;
                s.keys.push(key);
            });
            return ControlFlow::Break(());
        }
        let shortcut = game.edit_shortcut(event);
        let ctrl = game.keys.contains(&KeyCode::ControlLeft) || game.keys.contains(&KeyCode::ControlRight);
        let shift = reverse(game);
        let types = game.types_text();
        let mut edit = st(|s| s.edits.remove(&id));
        if let Some(e) = edit.as_mut() {
            if let Some(shortcut) = shortcut {
                e.shortcut(shortcut, &mut game.clipboard);
            } else if !e.key(key, ctrl, shift)
                && types
                && let Some(text) = event.text.as_deref()
            {
                insert(e, text);
            }
        }
        if let Some(e) = edit {
            st(|s| s.edits.insert(id, e));
        }
        return ControlFlow::Break(());
    }
    st(|s| s.keys.push(key));
    ControlFlow::Break(())
}

fn insert(e: &mut EditBox, typed: &str) {
    let allowed: String = typed.chars().filter(|&c| c != '\u{a7}' && c >= ' ' && c != '\u{7f}').collect();
    if !allowed.is_empty() {
        e.insert_text(&allowed);
    }
}

/// Text from the input method, into the focused edit box.
#[allow(dead_code)]
pub(super) fn hook_type(game: &mut Game<'_>, typed: &str) {
    if !is_open(game) {
        return;
    }
    st(|s| {
        if let Some(id) = s.focus.clone()
            && let Some(e) = s.edits.get_mut(&id)
        {
            insert(e, typed);
        }
    });
}

/// `pauses_game`: a mod's menu doesn't pause the game.
#[allow(dead_code)]
pub(super) fn hook_pauses(game: &Game<'_>) -> ControlFlow<bool> {
    if is_open(game) { ControlFlow::Break(false) } else { ControlFlow::Continue(()) }
}

/// Draws the screen with `build` (the Debug Options screen's draw).
#[allow(dead_code)]
pub(super) fn draw(game: &mut Game<'_>, batch: &mut HudBatch, build: impl FnOnce(&mut Game<'_>, &mut Ui<'_>)) {
    if !is_open(game) {
        return;
    }
    let mut ui = st(|s| Ui {
        width: batch.width,
        height: batch.height,
        cursor: game.cursor,
        presses: std::mem::take(&mut s.presses),
        mouse_down: s.mouse_down,
        released: std::mem::take(&mut s.released),
        wheel: std::mem::take(&mut s.wheel),
        keys: std::mem::take(&mut s.keys),
        shift: false,
        held: s.held.take(),
        scroll: std::mem::take(&mut s.scroll),
        edits: std::mem::take(&mut s.edits),
        focus: s.focus.take(),
        clicked_edit: false,
        clip: None,
        tooltip: Vec::new(),
        close: false,
        batch,
    });
    ui.shift = reverse(game);
    let pressed = !ui.presses.is_empty();
    ui::menu_background(ui.batch, &game.gui.atlas, 0.0, 0.0, ui.width, ui.height, true);
    build(game, &mut ui);
    if !ui.tooltip.is_empty() {
        let lines: Vec<Text> = ui.tooltip.iter().map(|l| Text::literal(l.as_str())).collect();
        ui::tooltip(ui.batch, &game.gui.atlas, &lines, ui.cursor);
    }
    // A press beside every edit box leaves the focused one.
    if pressed && !ui.clicked_edit {
        ui.focus = None;
    }
    if ui.released || !ui.mouse_down {
        ui.held = None;
    }
    let close = ui.close;
    let Ui { held, scroll, edits, focus, .. } = ui;
    st(|s| {
        s.held = held;
        s.scroll = scroll;
        s.edits = edits;
        s.focus = focus;
    });
    if close {
        close_screen(game);
    }
}

// ---------------------------------------------------------------- widgets

/// A rectangle in GUI pixels: x, y, width, height.
pub(super) type Rect = [f32; 4];

fn inside(p: (f32, f32), r: Rect) -> bool {
    p.0 >= r[0] && p.0 < r[0] + r[2] && p.1 >= r[1] && p.1 < r[1] + r[3]
}

fn color(argb: u32) -> u32 {
    let [a, r, g, b] = argb.to_be_bytes();
    rgba(r, g, b, a)
}

/// One frame of the screen: draw widgets top to bottom; each answers this frame's input.
pub(super) struct Ui<'b> {
    pub(super) batch: &'b mut HudBatch,
    /// The screen's size in GUI pixels.
    pub(super) width: f32,
    pub(super) height: f32,
    /// The pointer.
    pub(super) cursor: (f32, f32),
    /// Shift held (buttons may do more, sliders and choices step back).
    pub(super) shift: bool,
    presses: Vec<(f32, f32)>,
    mouse_down: bool,
    released: bool,
    wheel: f32,
    keys: Vec<KeyCode>,
    held: Option<String>,
    scroll: HashMap<String, f32>,
    edits: HashMap<String, EditBox>,
    focus: Option<String>,
    clicked_edit: bool,
    clip: Option<Rect>,
    tooltip: Vec<String>,
    close: bool,
}

#[allow(dead_code)]
impl Ui<'_> {
    /// The pointer is over `r` (and inside the scroll area being drawn).
    pub(super) fn hovered(&self, r: Rect) -> bool {
        inside(self.cursor, r) && self.clip.is_none_or(|c| inside(self.cursor, c))
    }

    /// A press this frame inside `r` (taken: one widget gets it).
    fn take_press(&mut self, r: Rect) -> bool {
        let clip = self.clip;
        if let Some(i) = self.presses.iter().position(|&p| inside(p, r) && clip.is_none_or(|c| inside(p, c))) {
            self.presses.remove(i);
            return true;
        }
        false
    }

    /// A press inside `r` this frame (for rows and areas that aren't buttons).
    pub(super) fn clicked(&mut self, r: Rect) -> bool {
        self.take_press(r)
    }

    /// A key pressed this frame that no edit box took (Enter and Tab also leave a box).
    pub(super) fn key(&mut self, key: KeyCode) -> bool {
        if let Some(i) = self.keys.iter().position(|&k| k == key) {
            self.keys.remove(i);
            return true;
        }
        false
    }

    /// Closes the screen after this frame.
    pub(super) fn close(&mut self) {
        self.close = true;
    }

    /// Lines shown beside the pointer at the end of the frame.
    pub(super) fn tooltip(&mut self, lines: &[&str]) {
        self.tooltip = lines.iter().map(|l| l.to_string()).collect();
    }

    pub(super) fn fill(&mut self, game: &Game<'_>, r: Rect, argb: u32) {
        self.batch.fill(&game.gui.atlas, r[0], r[1], r[2], r[3], color(argb));
    }

    /// An outline one pixel wide.
    pub(super) fn frame(&mut self, game: &Game<'_>, r: Rect, argb: u32) {
        let [x, y, w, h] = r;
        self.fill(game, [x, y, w, 1.0], argb);
        self.fill(game, [x, y + h - 1.0, w, 1.0], argb);
        self.fill(game, [x, y, 1.0, h], argb);
        self.fill(game, [x + w - 1.0, y, 1.0, h], argb);
    }

    pub(super) fn text_width(&self, game: &Game<'_>, text: &str) -> f32 {
        HudBatch::text_width(&game.gui.atlas, text)
    }

    /// Text with a shadow; `rgb` like 0xFFFFFF.
    pub(super) fn text(&mut self, game: &Game<'_>, x: f32, y: f32, text: &str, rgb: u32) {
        self.batch.text(&game.gui.atlas, text, x, y, color(0xFF00_0000 | rgb), true);
    }

    /// Text cut to `width` with "...".
    pub(super) fn text_fit(&mut self, game: &Game<'_>, x: f32, y: f32, width: f32, text: &str, rgb: u32) {
        let shown = self.fit(game, text, width);
        self.text(game, x, y, &shown, rgb);
    }

    pub(super) fn fit(&self, game: &Game<'_>, text: &str, width: f32) -> String {
        if self.text_width(game, text) <= width {
            return text.to_string();
        }
        let mut out = String::new();
        for c in text.chars() {
            out.push(c);
            if self.text_width(game, &format!("{out}...")) > width {
                out.pop();
                break;
            }
        }
        format!("{out}...")
    }

    pub(super) fn text_right(&mut self, game: &Game<'_>, right: f32, y: f32, text: &str, rgb: u32) {
        let w = self.text_width(game, text);
        self.text(game, right - w, y, text, rgb);
    }

    pub(super) fn text_centered(&mut self, game: &Game<'_>, cx: f32, y: f32, text: &str, rgb: u32) {
        let w = self.text_width(game, text);
        self.text(game, (cx - w / 2.0).floor(), y, text, rgb);
    }

    /// Text broken into lines of at most `width`; returns the height used (9 per line).
    pub(super) fn text_wrapped(&mut self, game: &Game<'_>, x: f32, y: f32, width: f32, text: &str, rgb: u32) -> f32 {
        let mut line = String::new();
        let mut dy = 0.0;
        for word in text.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && self.text_width(game, &candidate) > width {
                self.text(game, x, y + dy, &line, rgb);
                dy += 10.0;
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            self.text(game, x, y + dy, &line, rgb);
            dy += 10.0;
        }
        dy
    }

    /// The game's button; true when clicked.
    pub(super) fn button(&mut self, game: &Game<'_>, r: Rect, label: &str, active: bool) -> bool {
        let hover = self.hovered(r);
        ui::button(self.batch, &game.gui.atlas, r[0], r[1], r[2], r[3], &Text::literal(label), active, hover);
        active && self.take_press(r)
    }

    /// A button with a tooltip.
    pub(super) fn button_tip(&mut self, game: &Game<'_>, r: Rect, label: &str, active: bool, tip: &[&str]) -> bool {
        if self.hovered(r) {
            self.tooltip(tip);
        }
        self.button(game, r, label, active)
    }

    /// The game's slider at `value` (0 to 1), 20 high. Some(new value) while dragged.
    pub(super) fn slider(&mut self, game: &Game<'_>, id: &str, r: Rect, label: &str, value: f32) -> Option<f32> {
        let hover = self.hovered(r);
        if self.take_press(r) {
            self.held = Some(id.to_string());
        }
        let dragging = self.held.as_deref() == Some(id);
        let new = (dragging && (self.mouse_down || self.released)).then(|| ui::slider_value(r[0], r[2], self.cursor.0));
        let shown = new.unwrap_or(value);
        ui::slider(self.batch, &game.gui.atlas, r[0], r[1], r[2], &Text::literal(label), shown, hover, true, dragging);
        new.filter(|v| (v - value).abs() > f32::EPSILON)
    }

    /// The game's edit box `id`; its text. `hint` shows while it is empty and not focused.
    pub(super) fn edit(&mut self, game: &Game<'_>, id: &str, r: Rect, hint: &str, max_length: usize) -> String {
        let shift = self.shift;
        let clicked = self.take_press(r);
        if clicked {
            self.focus = Some(id.to_string());
            self.clicked_edit = true;
        }
        let focused = self.focus.as_deref() == Some(id);
        let e = self.edits.entry(id.to_string()).or_insert_with(|| {
            let mut e = EditBox::new(r[0], r[1], r[2], r[3]);
            e.focused = false;
            e
        });
        e.x = r[0];
        e.y = r[1];
        e.width = r[2];
        e.height = r[3];
        e.max_length = max_length;
        if focused && !e.focused {
            e.reset_blink();
        }
        e.focused = focused;
        if clicked {
            e.click(&game.gui.atlas, self.cursor.0, shift, false);
        }
        e.draw(self.batch, &game.gui.atlas, None);
        let value = e.value.clone();
        let (tx, ty) = (e.text_x(), e.text_y());
        if value.is_empty() && !focused {
            self.text(game, tx, ty, hint, 0x707070);
        }
        value
    }

    /// Replaces an edit box's text (when the mod changes what it shows).
    pub(super) fn set_edit(&mut self, id: &str, value: &str) {
        if let Some(e) = self.edits.get_mut(id) {
            e.set_value(value);
        } else {
            let mut e = EditBox::new(0.0, 0.0, 10.0, 20.0);
            e.focused = false;
            e.set_value(value);
            self.edits.insert(id.to_string(), e);
        }
    }

    pub(super) fn focused(&self, id: &str) -> bool {
        self.focus.as_deref() == Some(id)
    }

    /// An item's icon, `size` pixels (16 is the inventory's).
    pub(super) fn item(&mut self, game: &Game<'_>, x: f32, y: f32, item: &str, size: f32) -> bool {
        game.native_item(self.batch, item, [x, y], size)
    }

    /// Tabs across `r`: the current one is shown pressed. The tab clicked, if any.
    pub(super) fn tabs(&mut self, game: &Game<'_>, r: Rect, labels: &[&str], current: usize) -> Option<usize> {
        let n = labels.len().max(1) as f32;
        let w = ((r[2] - (n - 1.0) * 2.0) / n).floor();
        let mut clicked = None;
        for (i, label) in labels.iter().enumerate() {
            let x = r[0] + i as f32 * (w + 2.0);
            if self.button(game, [x, r[1], w, r[3]], label, i != current) {
                clicked = Some(i);
            }
        }
        clicked
    }

    /// Starts a scrolled area `id` over `r` with `content` pixels of rows: draw them from the
    /// returned top (y of the first row). The wheel scrolls it while the pointer is over it.
    pub(super) fn scroll_begin(&mut self, id: &str, r: Rect, content: f32) -> (f32, usize) {
        let max = (content - r[3]).max(0.0);
        let mut offset = self.scroll.get(id).copied().unwrap_or(0.0);
        if self.wheel != 0.0 && inside(self.cursor, r) {
            offset -= self.wheel * 12.0;
            self.wheel = 0.0;
        }
        // A press on the scroll bar's column jumps there.
        let bar = [r[0] + r[2] - 6.0, r[1], 6.0, r[3]];
        if max > 0.0 && self.take_press(bar) {
            offset = (self.cursor.1 - r[1]) / r[3] * content - r[3] / 2.0;
        }
        offset = offset.clamp(0.0, max);
        self.scroll.insert(id.to_string(), offset);
        self.clip = Some(r);
        (r[1] - offset, self.batch.vertices.len())
    }

    /// Ends the area: what was drawn is cut to it, and its scroll bar drawn.
    pub(super) fn scroll_end(&mut self, game: &Game<'_>, id: &str, r: Rect, content: f32, mark: usize) {
        self.batch.clip_since(mark, r[0], r[1], r[0] + r[2], r[1] + r[3]);
        self.clip = None;
        let offset = self.scroll.get(id).copied().unwrap_or(0.0);
        if content > r[3] {
            ui::scrollbar(self.batch, &game.gui.atlas, r[0] + r[2] - 6.0, r[1], r[1] + r[3], content, offset);
        }
    }

    /// Sets a scroll area back to its top.
    pub(super) fn scroll_reset(&mut self, id: &str) {
        self.scroll.remove(id);
    }

    /// A menu page's settings, actions and info lines laid out in `columns` columns of the
    /// game's widgets (toggles and choices as buttons, numbers as sliders) from `top` within
    /// `r` (x, y, width; height unused). Returns the event and the height used.
    pub(super) fn settings(
        &mut self,
        game: &Game<'_>,
        menu: &Menu,
        page: &str,
        store: &mut dyn Store,
        r: Rect,
        columns: usize,
    ) -> (Event, f32) {
        let Some(page) = menu.page_by_id(page) else { return (Event::None, 0.0) };
        let columns = columns.max(1);
        let gap = 4.0;
        let w = ((r[2] - gap * (columns as f32 - 1.0)) / columns as f32).floor();
        let mut event = Event::None;
        let (mut col, mut y) = (0usize, r[1]);
        let next = |col: &mut usize, y: &mut f32| {
            *col += 1;
            if *col == columns {
                *col = 0;
                *y += 24.0;
            }
        };
        for entry in &page.entries {
            let x = r[0] + col as f32 * (w + gap);
            match entry {
                Entry::Setting(s) => {
                    let label = menu.display(store, s);
                    match s.control {
                        Control::Number { .. } => {
                            let value = menu.fraction(store, &s.key) as f32;
                            if let Some(v) = self.slider(game, &s.key, [x, y, w, 20.0], &label, value)
                                && menu.set_fraction(store, &s.key, f64::from(v))
                            {
                                event = Event::Changed(s.key.clone());
                            }
                        }
                        _ => {
                            if self.button(game, [x, y, w, 20.0], &label, true) {
                                event = menu.step(store, &s.key, self.shift);
                            }
                        }
                    }
                    next(&mut col, &mut y);
                }
                Entry::Action { id, label } => {
                    if self.button(game, [x, y, w, 20.0], label, true) {
                        event = Event::Action(id.clone());
                    }
                    next(&mut col, &mut y);
                }
                Entry::Info(text) => {
                    if col != 0 {
                        col = 0;
                        y += 24.0;
                    }
                    self.text(game, r[0], y + 6.0, text, 0xA0A0A0);
                    y += 16.0;
                }
                Entry::Link { .. } | Entry::List { .. } => {}
            }
        }
        if col != 0 {
            y += 24.0;
        }
        (event, y - r[1])
    }
}
