//! A surface at one moment: the screen that shows, what the fields hold, what the variables
//! say. Everything a click, a key or a wheel does is decided here, and nothing here reads a clock
//! or the system.

use super::def::{Act, Cond, ItemDef, Kind, Place, ScreenDef, SurfaceDef, When};
use cua_action::ScrollDir;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Pixels one wheel notch moves a pane.
pub const NOTCH_PX: u32 = 40;

/// Why a surface could not be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceError {
    /// A `goto` or a start names a screen the surface lacks.
    #[error("no screen named {0}")]
    NoScreen(String),
}

/// A key the surfaces answer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
}

/// What a click landed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// An item, by its id (or its text when it has none).
    Item(String),
    /// Nothing that reacts.
    Nothing,
}

/// An item as it is placed on the window right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed<'a> {
    /// Its place among the screen's items.
    pub index: usize,
    /// Its definition.
    pub item: &'a ItemDef,
    /// Left, top, width and height of the part the viewport shows, in window pixels.
    pub rect: [u32; 4],
    /// Where the item's left edge is, which differs from its definition for a menu item.
    pub left: u32,
    /// Where the item's top edge is, which is above the viewport when it is cut off there.
    pub top: i64,
    /// Its text with the variables filled in.
    pub text: String,
    /// What a field holds.
    pub value: String,
}

/// One fake application, running.
#[derive(Debug, Clone)]
pub struct Surface {
    def: Arc<SurfaceDef>,
    screen: usize,
    vars: BTreeMap<String, String>,
    fields: BTreeMap<String, String>,
    focus: Option<usize>,
    scroll: u32,
    /// Where the context menu was opened, while it is open.
    menu: Option<[u32; 2]>,
}

impl Surface {
    /// The surface on its `start` screen, with `setup` already done.
    pub fn new(def: Arc<SurfaceDef>, start: &str, setup: &[Act]) -> Result<Surface, SurfaceError> {
        let screen = index_of(&def, start)?;
        let mut surface = Surface {
            def,
            screen,
            vars: BTreeMap::new(),
            fields: BTreeMap::new(),
            focus: None,
            scroll: 0,
            menu: None,
        };
        surface.focus = surface.first_focus();
        surface.run(setup);
        Ok(surface)
    }

    /// The app id of the window.
    pub fn app(&self) -> &str {
        &self.def.app
    }

    /// The window's size in pixels.
    pub fn size(&self) -> [u32; 2] {
        self.def.size
    }

    /// The window title now.
    pub fn title(&self) -> String {
        self.fill(&self.current().title)
    }

    /// The id of the screen that shows.
    pub fn screen_id(&self) -> &str {
        &self.current().id
    }

    /// The id of the field that has the focus.
    pub fn focused(&self) -> Option<&str> {
        self.focus
            .and_then(|i| self.current().items.get(i))
            .map(|item| item.id.as_str())
    }

    fn current(&self) -> &ScreenDef {
        &self.def.screens[self.screen]
    }

    /// Whether `cond` holds now.
    pub fn holds(&self, cond: &Cond) -> bool {
        let has = |map: &BTreeMap<String, String>, name: &str| {
            map.get(name).map(String::as_str).unwrap_or("").to_owned()
        };
        match cond {
            Cond::Screen(id) => self.screen_id() == id,
            Cond::Field([name, value]) => has(&self.fields, name) == *value,
            Cond::Has([name, text]) => {
                has(&self.fields, name).contains(text.as_str())
                    || has(&self.vars, name).contains(text.as_str())
            }
            Cond::Var([name, value]) => has(&self.vars, name) == *value,
            Cond::Like([name, text]) => {
                let text = text.to_lowercase();
                has(&self.fields, name).to_lowercase().contains(&text)
                    || has(&self.vars, name).to_lowercase().contains(&text)
            }
            Cond::Not(inner) => !self.holds(inner),
        }
    }

    /// The text with `{name}` replaced by the variable or the field of that name.
    pub fn fill(&self, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            out.push_str(&rest[..open]);
            let name = &rest[open + 1..open + close];
            let value = self.vars.get(name).or_else(|| self.fields.get(name));
            out.push_str(value.map(String::as_str).unwrap_or(""));
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        out
    }

    /// The items the viewport shows, back to front.
    pub fn visible(&self) -> Vec<Placed<'_>> {
        let screen = self.current();
        screen
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| self.place(screen, index, item))
            .collect()
    }

    fn place<'a>(&self, screen: &ScreenDef, index: usize, item: &'a ItemDef) -> Option<Placed<'a>> {
        if !item.show.iter().all(|c| self.holds(c)) {
            return None;
        }
        let [x, y, w, h] = item.rect;
        let (left, top) = match (item.place, screen.pane, self.menu) {
            (Place::Menu, _, None) => return None,
            (Place::Menu, _, Some([mx, my])) => {
                let [width, height] = self.def.size;
                (
                    mx.saturating_add(x).min(width.saturating_sub(w)),
                    i64::from(my.saturating_add(y).min(height.saturating_sub(h))),
                )
            }
            (Place::Pane, Some(pane), _) => (
                x,
                i64::from(pane.top) + i64::from(y) - i64::from(self.scroll),
            ),
            _ => (x, i64::from(y)),
        };
        let (from, to) = match (item.place, screen.pane) {
            (Place::Pane, Some(pane)) => (
                top.max(i64::from(pane.top)),
                (top + i64::from(h)).min(i64::from(pane.bottom)),
            ),
            _ => (top, top + i64::from(h)),
        };
        if to <= from {
            return None;
        }
        Some(Placed {
            index,
            item,
            rect: [
                left,
                u32::try_from(from).ok()?,
                w,
                u32::try_from(to - from).ok()?,
            ],
            left,
            top,
            text: self.fill(&item.text),
            value: self.fields.get(&item.id).cloned().unwrap_or_default(),
        })
    }

    /// The middle of the item named `id` as it shows now; `None` when it is not on the screen.
    pub fn centre_of(&self, id: &str) -> Option<[u32; 2]> {
        self.visible()
            .into_iter()
            .find(|p| p.item.id == id)
            .map(|p| [p.rect[0] + p.rect[2] / 2, p.rect[1] + p.rect[3] / 2])
    }

    /// The topmost item that reacts at a window pixel, with its place among the screen's items.
    fn item_at(&self, x: u32, y: u32) -> Option<(usize, ItemDef)> {
        self.visible()
            .into_iter()
            .rev()
            .find(|p| reacts(p.item.kind) && inside(p.rect, x, y))
            .map(|p| (p.index, p.item.clone()))
    }

    /// A click at a window pixel. While the context menu is open, a click outside its items only
    /// closes it.
    pub fn click(&mut self, x: u32, y: u32) -> Hit {
        let hit = self.item_at(x, y);
        let menu_was_open = self.menu.take().is_some();
        let Some((index, item)) = hit else {
            self.focus = None;
            return Hit::Nothing;
        };
        if menu_was_open && item.place != Place::Menu {
            return Hit::Nothing;
        }
        let name = name_of(&item);
        match item.kind {
            Kind::Field => self.focus = Some(index),
            Kind::Check => {
                self.focus = None;
                let on = self.vars.get(&item.id).map(String::as_str) == Some("on");
                self.vars
                    .insert(item.id.clone(), if on { "off" } else { "on" }.to_owned());
                self.run(&item.click);
            }
            Kind::Button | Kind::Row | Kind::Label => {
                self.focus = None;
                self.run(&item.click);
            }
        }
        Hit::Item(name)
    }

    /// A right click at a window pixel: on an item that has a context menu, it runs the item's
    /// context acts and opens the menu at the pointer; anywhere else it only closes an open menu.
    pub fn context_click(&mut self, x: u32, y: u32) -> Hit {
        self.menu = None;
        let Some((_, item)) = self.item_at(x, y).filter(|(_, i)| !i.context.is_empty()) else {
            return Hit::Nothing;
        };
        self.focus = None;
        self.run(&item.context);
        self.menu = Some([x, y]);
        Hit::Item(name_of(&item))
    }

    /// The standard action with this slug ("copy"), as a shortcut: does what the screen says for
    /// it, and nothing when it says nothing.
    pub fn shortcut(&mut self, slug: &str) {
        let acts = self
            .current()
            .shortcuts
            .iter()
            .find(|s| s.action == slug)
            .map(|s| s.then.clone());
        if let Some(acts) = acts {
            self.run(&acts);
        }
    }

    /// Typed text goes to the field with the focus; with none, nowhere. A newline is the Enter
    /// key and a tab the Tab key, as on a real keyboard.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '\n' | '\r' => self.press(Stroke::Enter),
                '\t' => self.press(Stroke::Tab),
                c => self.put(c),
            }
        }
    }

    fn put(&mut self, c: char) {
        let Some(id) = self.focused().map(str::to_owned) else {
            return;
        };
        self.fields.entry(id).or_default().push(c);
    }

    /// A key.
    pub fn press(&mut self, stroke: Stroke) {
        match stroke {
            Stroke::Enter => {
                let acts = match self.focus.and_then(|i| self.current().items.get(i)) {
                    Some(field) => field.enter.clone(),
                    None => self.current().enter.clone(),
                };
                self.run(&acts);
            }
            Stroke::Escape => {
                if self.menu.take().is_some() {
                    return;
                }
                let acts = self.current().escape.clone();
                self.run(&acts);
            }
            Stroke::Backspace => {
                if let Some(id) = self.focused().map(str::to_owned) {
                    self.fields.entry(id).or_default().pop();
                }
            }
            Stroke::Tab => self.focus = self.next_field(),
        }
    }

    /// The field the current screen gives the focus to when it shows.
    fn first_focus(&self) -> Option<usize> {
        let screen = self.current();
        let id = screen.focus.as_deref()?;
        screen
            .items
            .iter()
            .position(|i| i.kind == Kind::Field && i.id == id)
    }

    fn next_field(&self) -> Option<usize> {
        let items = &self.current().items;
        let after = self.focus.map_or(0, |i| i + 1);
        let fields = |range: std::ops::Range<usize>| {
            range.into_iter().find(|&i| items[i].kind == Kind::Field)
        };
        fields(after..items.len()).or_else(|| fields(0..items.len()))
    }

    /// The wheel, over a pane.
    pub fn scroll(&mut self, dir: ScrollDir, notches: u16) {
        let Some(pane) = self.current().pane else {
            return;
        };
        let room = pane.content.saturating_sub(pane.bottom - pane.top);
        let step = NOTCH_PX * u32::from(notches);
        self.scroll = match dir {
            ScrollDir::Down => (self.scroll + step).min(room),
            ScrollDir::Up => self.scroll.saturating_sub(step),
            ScrollDir::Left | ScrollDir::Right => self.scroll,
        };
    }

    /// Carries out acts in order.
    pub fn run(&mut self, acts: &[Act]) {
        for act in acts {
            match act {
                Act::Goto(id) => self.goto(id),
                Act::Set([name, value]) => {
                    let value = self.fill(value);
                    self.vars.insert(name.clone(), value);
                }
                Act::Copy([field, var]) => {
                    let value = self.fields.get(field).cloned().unwrap_or_default();
                    self.vars.insert(var.clone(), value);
                }
                Act::Clear(field) => {
                    self.fields.remove(field);
                }
                Act::When(When {
                    cond,
                    then,
                    otherwise,
                }) => {
                    let branch = if self.holds(cond) { then } else { otherwise };
                    let branch = branch.clone();
                    self.run(&branch);
                }
            }
        }
    }

    fn goto(&mut self, id: &str) {
        if let Ok(screen) = index_of(&self.def, id) {
            self.screen = screen;
            self.menu = None;
            self.focus = self.first_focus();
            self.scroll = 0;
        }
    }
}

fn index_of(def: &SurfaceDef, id: &str) -> Result<usize, SurfaceError> {
    def.screens
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| SurfaceError::NoScreen(id.to_owned()))
}

fn name_of(item: &ItemDef) -> String {
    if item.id.is_empty() {
        item.text.clone()
    } else {
        item.id.clone()
    }
}

fn reacts(kind: Kind) -> bool {
    !matches!(kind, Kind::Label)
}

fn inside(rect: [u32; 4], x: u32, y: u32) -> bool {
    x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
}
