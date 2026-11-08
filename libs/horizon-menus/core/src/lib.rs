//! # horizon-menus
//!
//! Menus and settings for Horizon native mods, shown on the mod's page in Options > Mods.
//!
//! Horizon gives a native mod a page of buttons (`native_settings` / `native_settings_click`) and
//! asks for the labels again after every click. This crate turns that into a menu:
//!
//! - a **schema** of settings (toggles, choices, numbers) with defaults, read and written through a
//!   [`Store`] (the game side's store is Horizon's `native_setting` / `native_set_setting`);
//! - **pages** with links between them and a "< Back" row;
//! - **actions** the mod runs, **info** rows, and **lists** the mod fills when the page is drawn
//!   (files, status lines), paged when long.
//!
//! Nothing here knows the game: the mod's glue passes a [`Store`] and its lists in, and gets
//! [`Event`]s back. Shift-click (`reverse`) steps numbers and choices backwards.
//!
//! ```
//! use horizon_menus::{Event, Menu, Nav, Store};
//! use std::collections::HashMap;
//!
//! let menu = Menu::new("My mod")
//!     .page("main", "My mod", |p| {
//!         p.toggle("enabled", "Enabled", true);
//!         p.percent("opacity", "Opacity", 0.5, 0.1, 1.0, 0.1);
//!         p.link("more", "More settings");
//!         p.action("hello", "Say hello");
//!     })
//!     .page("more", "More settings", |p| {
//!         p.choice("mode", "Mode", &[("fast", "Fast"), ("pretty", "Pretty")], "fast");
//!     });
//! let mut store: HashMap<String, String> = HashMap::new();
//! let mut nav = Nav::default();
//! let rows = menu.render(&mut nav, &store, &mut |_| Vec::new());
//! assert_eq!(rows[0], "Enabled: ON");
//! assert_eq!(menu.click(&mut nav, &mut store, 0, false), Event::Changed("enabled".into()));
//! assert!(!menu.bool(&store, "enabled"));
//! ```

use std::collections::HashMap;

/// Where the values live: the game side's is Horizon's per-mod settings.
pub trait Store {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&mut self, key: &str, value: &str);
}

impl Store for HashMap<String, String> {
    fn get(&self, key: &str) -> Option<String> {
        HashMap::get(self, key).cloned()
    }
    fn set(&mut self, key: &str, value: &str) {
        self.insert(key.to_string(), value.to_string());
    }
}

/// How a number is shown.
#[derive(Clone, Debug, PartialEq)]
pub enum Unit {
    /// With this many decimals and a suffix (`" ticks"`, `" blocks"`; may be empty).
    Plain { decimals: u8, suffix: String },
    /// A fraction shown as a percentage.
    Percent,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Toggle { default: bool },
    /// Stored as the value (the first of each pair), shown as the label.
    Choice { options: Vec<(String, String)>, default: String },
    /// Clicks step up by `step` and wrap from `max` to `min` (Shift: down).
    Number { min: f64, max: f64, step: f64, default: f64, unit: Unit },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    pub key: String,
    pub label: String,
    pub control: Control,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Setting(Setting),
    /// A row that opens another page.
    Link { page: String, label: String },
    /// A row whose click the mod handles ([`Event::Action`]).
    Action { id: String, label: String },
    /// A line of text; clicking it does nothing.
    Info(String),
    /// Rows the mod gives when the page is drawn: `(label, action id)`, an empty id for a
    /// line that does nothing. More than `per_page` are paged.
    List { id: String, per_page: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub entries: Vec<Entry>,
}

/// Builds a page's entries ([`Menu::page`]).
pub struct PageBuilder<'a> {
    entries: &'a mut Vec<Entry>,
}

impl PageBuilder<'_> {
    pub fn toggle(&mut self, key: &str, label: &str, default: bool) -> &mut Self {
        self.setting(key, label, Control::Toggle { default })
    }
    /// `options`: (stored value, label).
    pub fn choice(&mut self, key: &str, label: &str, options: &[(&str, &str)], default: &str) -> &mut Self {
        let options = options.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
        self.setting(key, label, Control::Choice { options, default: default.to_string() })
    }
    pub fn number(
        &mut self,
        key: &str,
        label: &str,
        default: f64,
        (min, max, step): (f64, f64, f64),
        decimals: u8,
        suffix: &str,
    ) -> &mut Self {
        let unit = Unit::Plain { decimals, suffix: suffix.to_string() };
        self.setting(key, label, Control::Number { min, max, step, default, unit })
    }
    pub fn percent(&mut self, key: &str, label: &str, default: f64, min: f64, max: f64, step: f64) -> &mut Self {
        self.setting(key, label, Control::Number { min, max, step, default, unit: Unit::Percent })
    }
    pub fn link(&mut self, page: &str, label: &str) -> &mut Self {
        self.entries.push(Entry::Link { page: page.to_string(), label: label.to_string() });
        self
    }
    pub fn action(&mut self, id: &str, label: &str) -> &mut Self {
        self.entries.push(Entry::Action { id: id.to_string(), label: label.to_string() });
        self
    }
    pub fn info(&mut self, text: &str) -> &mut Self {
        self.entries.push(Entry::Info(text.to_string()));
        self
    }
    pub fn list(&mut self, id: &str, per_page: usize) -> &mut Self {
        self.entries.push(Entry::List { id: id.to_string(), per_page: per_page.max(1) });
        self
    }
    fn setting(&mut self, key: &str, label: &str, control: Control) -> &mut Self {
        self.entries.push(Entry::Setting(Setting { key: key.to_string(), label: label.to_string(), control }));
        self
    }
}

/// What a row does when clicked (kept from the last [`Menu::render`]).
#[derive(Clone, Debug, PartialEq)]
enum Row {
    Nothing,
    Back,
    Setting(String),
    Open(String),
    Action(String),
    /// A list's page: its id and the page to show.
    Turn(String, usize),
}

/// Where the player is in the menu: the open pages, each list's page, and what the rows on
/// screen do. One per mod, kept by its glue.
#[derive(Clone, Debug, Default)]
pub struct Nav {
    stack: Vec<String>,
    list_pages: HashMap<String, usize>,
    rows: Vec<Row>,
}

impl Nav {
    /// Back to the first page.
    pub fn reset(&mut self) {
        self.stack.clear();
        self.list_pages.clear();
    }
    /// Opens `page` on top of the current one.
    pub fn open(&mut self, page: &str) {
        self.stack.push(page.to_string());
    }
    /// The id of the page shown, `None` for the first page.
    pub fn current(&self) -> Option<&str> {
        self.stack.last().map(String::as_str)
    }
}

/// What a click asks of the mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Nothing for the mod to do (navigation, an info row).
    None,
    /// A setting got a new value (already stored).
    Changed(String),
    /// An action row (or a list row) was clicked.
    Action(String),
}

/// A mod's menu: its pages, the first one shown first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    pub title: String,
    pub pages: Vec<Page>,
}

impl Menu {
    pub fn new(title: &str) -> Self {
        Self { title: title.to_string(), pages: Vec::new() }
    }

    pub fn page(mut self, id: &str, title: &str, build: impl FnOnce(&mut PageBuilder<'_>)) -> Self {
        let mut entries = Vec::new();
        build(&mut PageBuilder { entries: &mut entries });
        self.pages.push(Page { id: id.to_string(), title: title.to_string(), entries });
        self
    }

    fn find_page(&self, id: Option<&str>) -> Option<&Page> {
        match id {
            None => self.pages.first(),
            Some(id) => self.pages.iter().find(|p| p.id == id),
        }
    }

    /// The setting `key`, on any page.
    pub fn setting(&self, key: &str) -> Option<&Setting> {
        self.pages.iter().flat_map(|p| &p.entries).find_map(|e| match e {
            Entry::Setting(s) if s.key == key => Some(s),
            _ => None,
        })
    }

    // ---- reading values (the default when unset or unreadable)

    pub fn bool(&self, store: &dyn Store, key: &str) -> bool {
        let default = matches!(self.setting(key), Some(Setting { control: Control::Toggle { default: true }, .. }));
        store.get(key).map_or(default, |v| match v.as_str() {
            "true" | "on" | "1" => true,
            "false" | "off" | "0" => false,
            _ => default,
        })
    }

    pub fn number(&self, store: &dyn Store, key: &str) -> f64 {
        let Some(Setting { control: Control::Number { min, max, default, .. }, .. }) = self.setting(key) else {
            return store.get(key).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        };
        store.get(key).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).map_or(*default, |v| v.clamp(*min, *max))
    }

    /// The stored value of a choice (one of its options' values).
    pub fn choice(&self, store: &dyn Store, key: &str) -> String {
        let Some(Setting { control: Control::Choice { options, default }, .. }) = self.setting(key) else {
            return store.get(key).unwrap_or_default();
        };
        store.get(key).filter(|v| options.iter().any(|(o, _)| o == v)).unwrap_or_else(|| default.clone())
    }

    /// A setting's row text: `Label: value`.
    pub fn display(&self, store: &dyn Store, s: &Setting) -> String {
        match &s.control {
            Control::Toggle { .. } => format!("{}: {}", s.label, if self.bool(store, &s.key) { "ON" } else { "OFF" }),
            Control::Choice { options, .. } => {
                let v = self.choice(store, &s.key);
                let shown = options.iter().find(|(o, _)| *o == v).map_or(v.as_str(), |(_, l)| l.as_str());
                format!("{}: {shown}", s.label)
            }
            Control::Number { unit, .. } => {
                let v = self.number(store, &s.key);
                match unit {
                    Unit::Percent => format!("{}: {}%", s.label, (v * 100.0).round()),
                    Unit::Plain { decimals, suffix } => {
                        format!("{}: {:.*}{suffix}", s.label, usize::from(*decimals), v)
                    }
                }
            }
        }
    }

    // ---- the page

    /// The labels of the page shown, top to bottom; `lists` gives each list's rows.
    pub fn render(
        &self,
        nav: &mut Nav,
        store: &dyn Store,
        lists: &mut dyn FnMut(&str) -> Vec<(String, String)>,
    ) -> Vec<String> {
        // A page that is gone (the mod changed): start over.
        if self.find_page(nav.current()).is_none() {
            nav.reset();
        }
        let mut labels = Vec::new();
        let mut rows = Vec::new();
        let Some(page) = self.find_page(nav.current()) else {
            nav.rows = rows;
            return labels;
        };
        if !nav.stack.is_empty() {
            let parent = self.find_page(nav.stack.iter().rev().nth(1).map(String::as_str));
            labels.push(format!("< Back to {}", parent.map_or(self.title.as_str(), |p| p.title.as_str())));
            rows.push(Row::Back);
            labels.push(format!("== {} ==", page.title));
            rows.push(Row::Nothing);
        }
        for entry in &page.entries {
            match entry {
                Entry::Setting(s) => {
                    labels.push(self.display(store, s));
                    rows.push(Row::Setting(s.key.clone()));
                }
                Entry::Link { page, label } => {
                    labels.push(format!("{label} >"));
                    rows.push(Row::Open(page.clone()));
                }
                Entry::Action { id, label } => {
                    labels.push(label.clone());
                    rows.push(Row::Action(id.clone()));
                }
                Entry::Info(text) => {
                    labels.push(text.clone());
                    rows.push(Row::Nothing);
                }
                Entry::List { id, per_page } => {
                    let items = lists(id);
                    let pages = items.len().div_ceil(*per_page).max(1);
                    let at = nav.list_pages.get(id).copied().unwrap_or(0).min(pages - 1);
                    nav.list_pages.insert(id.clone(), at);
                    if at > 0 {
                        labels.push(format!("< Previous ({}/{pages})", at));
                        rows.push(Row::Turn(id.clone(), at - 1));
                    }
                    for (label, action) in items.into_iter().skip(at * per_page).take(*per_page) {
                        labels.push(label);
                        rows.push(if action.is_empty() { Row::Nothing } else { Row::Action(action) });
                    }
                    if at + 1 < pages {
                        labels.push(format!("Next ({}/{pages}) >", at + 2));
                        rows.push(Row::Turn(id.clone(), at + 1));
                    }
                }
            }
        }
        nav.rows = rows;
        labels
    }

    /// A click on row `index` of the last [`render`](Self::render). `reverse` (Shift held) steps
    /// numbers and choices backwards.
    pub fn click(&self, nav: &mut Nav, store: &mut dyn Store, index: usize, reverse: bool) -> Event {
        let Some(row) = nav.rows.get(index).cloned() else { return Event::None };
        match row {
            Row::Nothing => Event::None,
            Row::Back => {
                nav.stack.pop();
                Event::None
            }
            Row::Open(page) => {
                nav.open(&page);
                Event::None
            }
            Row::Turn(list, at) => {
                nav.list_pages.insert(list, at);
                Event::None
            }
            Row::Action(id) => Event::Action(id),
            Row::Setting(key) => self.step(store, &key, reverse),
        }
    }

    /// The page `id`.
    pub fn page_by_id(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    /// A number's place in its range, 0 to 1 (a slider's handle); 0 for other settings.
    pub fn fraction(&self, store: &dyn Store, key: &str) -> f64 {
        match self.setting(key) {
            Some(Setting { control: Control::Number { min, max, .. }, .. }) if max > min => {
                ((self.number(store, key) - min) / (max - min)).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    /// Sets a number from a slider's place (0 to 1), snapped to its steps. Whether it changed.
    pub fn set_fraction(&self, store: &mut dyn Store, key: &str, fraction: f64) -> bool {
        let Some(Setting { control: Control::Number { min, max, step, .. }, .. }) = self.setting(key) else {
            return false;
        };
        let raw = min + (max - min) * fraction.clamp(0.0, 1.0);
        let snapped = if *step > 0.0 { min + ((raw - min) / step).round() * step } else { raw };
        let value = format_number(snapped.clamp(*min, *max));
        if store.get(key).as_deref() == Some(value.as_str()) {
            return false;
        }
        store.set(key, &value);
        true
    }

    /// One click on a setting: a toggle flips, a choice and a number step (backwards with
    /// `reverse`, wrapping at the ends).
    pub fn step(&self, store: &mut dyn Store, key: &str, reverse: bool) -> Event {
        let key = key.to_string();
        let Some(s) = self.setting(&key) else { return Event::None };
        let value = match &s.control {
            Control::Toggle { .. } => (!self.bool(store, &key)).to_string(),
            Control::Choice { options, .. } => {
                let v = self.choice(store, &key);
                let i = options.iter().position(|(o, _)| *o == v).unwrap_or(0);
                let n = options.len().max(1);
                let next = if reverse { (i + n - 1) % n } else { (i + 1) % n };
                options.get(next).map_or(v, |(o, _)| o.clone())
            }
            Control::Number { min, max, step, .. } => {
                let v = self.number(store, &key);
                let eps = step.abs() * 1e-6;
                let next = if reverse {
                    if v - step < min - eps { *max } else { v - step }
                } else if v + step > max + eps {
                    *min
                } else {
                    v + step
                };
                // Whole steps from min, so 0.1 + 0.2 doesn't drift.
                let snapped = if *step > 0.0 { min + ((next - min) / step).round() * step } else { next };
                format_number(snapped.clamp(*min, *max))
            }
        };
        store.set(&key, &value);
        Event::Changed(key)
    }
}

fn format_number(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        Menu::new("Test")
            .page("main", "Test", |p| {
                p.toggle("on", "On", true);
                p.choice("mode", "Mode", &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")], "b");
                p.percent("alpha", "Alpha", 0.5, 0.1, 1.0, 0.1);
                p.number("radius", "Radius", 32.0, (16.0, 64.0, 16.0), 0, " blocks");
                p.link("sub", "Sub");
                p.action("go", "Go");
                p.info("Just text");
            })
            .page("sub", "Sub page", |p| {
                p.list("files", 2);
            })
    }

    #[test]
    fn labels_and_defaults() {
        let m = menu();
        let store: HashMap<String, String> = HashMap::new();
        let mut nav = Nav::default();
        let rows = m.render(&mut nav, &store, &mut |_| Vec::new());
        assert_eq!(
            rows,
            ["On: ON", "Mode: Beta", "Alpha: 50%", "Radius: 32 blocks", "Sub >", "Go", "Just text"]
        );
        assert!(m.bool(&store, "on"));
        assert_eq!(m.choice(&store, "mode"), "b");
        assert_eq!(m.number(&store, "radius"), 32.0);
    }

    #[test]
    fn clicks_change_and_wrap() {
        let m = menu();
        let mut store: HashMap<String, String> = HashMap::new();
        let mut nav = Nav::default();
        m.render(&mut nav, &store, &mut |_| Vec::new());
        assert_eq!(m.click(&mut nav, &mut store, 0, false), Event::Changed("on".into()));
        assert_eq!(store["on"], "false");
        m.click(&mut nav, &mut store, 1, false);
        assert_eq!(m.choice(&store, "mode"), "c");
        m.click(&mut nav, &mut store, 1, false);
        assert_eq!(m.choice(&store, "mode"), "a", "wraps");
        m.click(&mut nav, &mut store, 1, true);
        assert_eq!(m.choice(&store, "mode"), "c", "shift goes back");
        for _ in 0..5 {
            m.click(&mut nav, &mut store, 2, false);
        }
        assert_eq!(store["alpha"], "1", "0.5 + 5 x 0.1 without drift");
        m.click(&mut nav, &mut store, 2, false);
        assert_eq!(store["alpha"], "0.1", "wraps to min");
        m.click(&mut nav, &mut store, 2, true);
        assert_eq!(store["alpha"], "1", "shift wraps to max");
        assert_eq!(m.click(&mut nav, &mut store, 5, false), Event::Action("go".into()));
        assert_eq!(m.click(&mut nav, &mut store, 6, false), Event::None);
    }

    #[test]
    fn bad_values_fall_back() {
        let m = menu();
        let mut store: HashMap<String, String> = HashMap::new();
        store.set("radius", "9999");
        store.set("mode", "nope");
        store.set("on", "maybe");
        assert_eq!(m.number(&store, "radius"), 64.0, "clamped");
        assert_eq!(m.choice(&store, "mode"), "b");
        assert!(m.bool(&store, "on"));
    }

    #[test]
    fn pages_back_and_lists() {
        let m = menu();
        let mut store: HashMap<String, String> = HashMap::new();
        let mut nav = Nav::default();
        let files = |_: &str| (1..=5).map(|i| (format!("file{i}"), format!("load:file{i}"))).collect::<Vec<_>>();
        m.render(&mut nav, &store, &mut |id| files(id));
        m.click(&mut nav, &mut store, 4, false); // Sub >
        let rows = m.render(&mut nav, &store, &mut |id| files(id));
        assert_eq!(rows, ["< Back to Test", "== Sub page ==", "file1", "file2", "Next (2/3) >"]);
        assert_eq!(m.click(&mut nav, &mut store, 3, false), Event::Action("load:file2".into()));
        m.click(&mut nav, &mut store, 4, false); // next
        let rows = m.render(&mut nav, &store, &mut |id| files(id));
        assert_eq!(rows, ["< Back to Test", "== Sub page ==", "< Previous (1/3)", "file3", "file4", "Next (3/3) >"]);
        m.click(&mut nav, &mut store, 0, false); // back
        assert_eq!(nav.current(), None);
        let rows = m.render(&mut nav, &store, &mut |id| files(id));
        assert_eq!(rows[0], "On: ON");
    }

    #[test]
    fn a_removed_page_resets() {
        let m = menu();
        let store: HashMap<String, String> = HashMap::new();
        let mut nav = Nav::default();
        nav.open("gone");
        let rows = m.render(&mut nav, &store, &mut |_| Vec::new());
        assert_eq!(rows[0], "On: ON");
    }

    #[test]
    fn sliders_snap() {
        let m = menu();
        let mut store: HashMap<String, String> = HashMap::new();
        assert_eq!(m.fraction(&store, "radius"), 1.0 / 3.0);
        assert!(m.set_fraction(&mut store, "radius", 0.9));
        assert_eq!(store["radius"], "64", "snapped to 16-steps: 16 + 0.9 x 48 = 59.2 -> 64");
        assert!(!m.set_fraction(&mut store, "radius", 0.95), "same value: no change");
        assert!(m.set_fraction(&mut store, "alpha", 0.0));
        assert_eq!(store["alpha"], "0.1");
        assert!(!m.set_fraction(&mut store, "on", 0.5), "not a number");
        assert_eq!(m.step(&mut store, "on", false), Event::Changed("on".into()));
        assert_eq!(m.display(&store, m.setting("on").unwrap()), "On: OFF");
    }
}
