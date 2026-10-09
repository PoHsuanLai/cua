//! A surface drawn as pixels: flat rectangles and bitmap text, so a vision model can read the
//! screens it is evaluated on. The same surface always draws the same bytes.

use super::def::{Echo, Kind};
use super::state::{Placed, Surface};
use crate::font_data::GLYPHS;
use cua_action::{DeviceSize, PixelFormat};
use maskcap::PixelBuf;
use std::sync::Arc;

/// The height of the window's title bar; screens start their content below it.
pub const TITLE_BAR: u32 = 40;
/// Pixels of the screen per pixel of the font.
const SCALE: u32 = 2;
/// A character cell.
const CELL_W: u32 = 8 * SCALE;
const CELL_H: u32 = 16 * SCALE;

type Rgb = [u8; 3];

const BACKGROUND: Rgb = [244, 244, 246];
const TITLE: Rgb = [58, 60, 72];
const INK: Rgb = [28, 28, 34];
const WHITE: Rgb = [255, 255, 255];
const BUTTON: Rgb = [38, 98, 204];
const BORDER: Rgb = [140, 140, 150];
const FOCUS: Rgb = [20, 120, 255];
const CHECKED: Rgb = [38, 150, 80];

/// A rectangle: left, top, right, bottom (exclusive).
#[derive(Debug, Clone, Copy)]
struct Clip([u32; 4]);

struct Canvas {
    w: u32,
    h: u32,
    data: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32, ground: Rgb) -> Canvas {
        let px = [ground[0], ground[1], ground[2], 0xff];
        Canvas {
            w,
            h,
            data: px.repeat((w * h) as usize),
        }
    }

    fn whole(&self) -> Clip {
        Clip([0, 0, self.w, self.h])
    }

    fn put(&mut self, x: u32, y: u32, rgb: Rgb, clip: Clip) {
        let Clip([l, t, r, b]) = clip;
        if x < l || x >= r.min(self.w) || y < t || y >= b.min(self.h) {
            return;
        }
        let at = ((y * self.w + x) * 4) as usize;
        self.data[at..at + 3].copy_from_slice(&rgb);
    }

    fn fill(&mut self, rect: [u32; 4], rgb: Rgb, clip: Clip) {
        for y in rect[1]..rect[1] + rect[3] {
            for x in rect[0]..rect[0] + rect[2] {
                self.put(x, y, rgb, clip);
            }
        }
    }

    fn frame(&mut self, rect: [u32; 4], width: u32, rgb: Rgb, clip: Clip) {
        let [x, y, w, h] = rect;
        self.fill([x, y, w, width], rgb, clip);
        self.fill([x, y + h.saturating_sub(width), w, width], rgb, clip);
        self.fill([x, y, width, h], rgb, clip);
        self.fill([x + w.saturating_sub(width), y, width, h], rgb, clip);
    }

    fn text(&mut self, at: [u32; 2], text: &str, rgb: Rgb, clip: Clip) {
        for (n, c) in text.chars().enumerate() {
            let glyph = &GLYPHS[glyph_of(c)];
            let left = at[0] + n as u32 * CELL_W;
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8u32 {
                    if bits >> (7 - col) & 1 == 1 {
                        let rect = [left + col * SCALE, at[1] + row as u32 * SCALE, SCALE, SCALE];
                        self.fill(rect, rgb, clip);
                    }
                }
            }
        }
    }
}

fn glyph_of(c: char) -> usize {
    match c {
        ' '..='~' => c as usize - 32,
        _ => '?' as usize - 32,
    }
}

/// `text` broken into lines of at most `room` characters, at spaces where it can.
fn wrap(text: &str, room: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let longer = line.chars().count() + 1 + word.chars().count() > room;
        if !line.is_empty() && longer {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    lines
}

/// How many characters fit in `width` pixels.
fn columns(width: u32) -> usize {
    (width / CELL_W) as usize
}

/// The window as the model sees it.
pub fn render(surface: &Surface) -> PixelBuf {
    let [w, h] = surface.size();
    let mut canvas = Canvas::new(w, h, BACKGROUND);
    let all = canvas.whole();
    canvas.fill([0, 0, w, TITLE_BAR], TITLE, all);
    canvas.text([12, (TITLE_BAR - CELL_H) / 2], &surface.title(), WHITE, all);
    let focused = surface.focused().map(str::to_owned);
    for placed in surface.visible() {
        let clip = Clip([0, placed.rect[1], w, placed.rect[1] + placed.rect[3]]);
        let has_focus =
            focused.as_deref() == Some(placed.item.id.as_str()) && !placed.item.id.is_empty();
        draw(&mut canvas, &placed, has_focus, surface, clip);
    }
    PixelBuf {
        format: PixelFormat::Xbgr8888,
        size: DeviceSize { w, h },
        stride: w * 4,
        data: Arc::from(canvas.data),
    }
}

fn draw(canvas: &mut Canvas, placed: &Placed<'_>, focus: bool, surface: &Surface, clip: Clip) {
    let [x, _, w, h] = placed.item.rect;
    let top = u32::try_from(placed.top).unwrap_or(0);
    let rect = [x, top, w, h];
    let pad = (h.saturating_sub(CELL_H)) / 2;
    match placed.item.kind {
        Kind::Label => {
            let room = columns(if w == 0 {
                canvas.w - x
            } else {
                w.min(canvas.w - x)
            });
            for (n, line) in wrap(&placed.text, room).iter().enumerate() {
                canvas.text([x, top + n as u32 * (CELL_H + 4)], line, INK, clip);
            }
        }
        Kind::Button => {
            canvas.fill(rect, BUTTON, clip);
            let shown = placed.text.chars().count().min(columns(w)) as u32;
            let left = x + (w.saturating_sub(shown * CELL_W)) / 2;
            canvas.text([left, top + pad], &placed.text, WHITE, clip);
        }
        Kind::Row => {
            canvas.fill(rect, WHITE, clip);
            canvas.frame(rect, 2, BORDER, clip);
            canvas.text([x + 10, top + pad], &placed.text, INK, clip);
        }
        Kind::Check => {
            let on = surface.holds(&super::def::Cond::Var([
                placed.item.id.clone(),
                "on".into(),
            ]));
            let boxed = [x, top + pad, CELL_H, CELL_H];
            canvas.fill(boxed, if on { CHECKED } else { WHITE }, clip);
            canvas.frame(boxed, 2, BORDER, clip);
            let mark = if on { "x" } else { " " };
            canvas.text([x + (CELL_H - CELL_W) / 2, top + pad], mark, WHITE, clip);
            canvas.text([x + CELL_H + 12, top + pad], &placed.text, INK, clip);
        }
        Kind::Field => draw_field(canvas, placed, rect, focus, clip),
    }
}

fn draw_field(canvas: &mut Canvas, placed: &Placed<'_>, rect: [u32; 4], focus: bool, clip: Clip) {
    let [x, top, w, h] = rect;
    if !placed.text.is_empty() {
        canvas.text(
            [x, top.saturating_sub(CELL_H + 2)],
            &placed.text,
            INK,
            Clip([0, 0, canvas.w, canvas.h]),
        );
    }
    canvas.fill(rect, WHITE, clip);
    canvas.frame(
        rect,
        if focus { 4 } else { 2 },
        if focus { FOCUS } else { BORDER },
        clip,
    );
    let mut shown: String = match placed.item.echo {
        Echo::Plain => placed.value.clone(),
        Echo::Masked => "*".repeat(placed.value.chars().count()),
    };
    if focus {
        shown.push('|');
    }
    let room = columns(w.saturating_sub(20));
    let skip = shown.chars().count().saturating_sub(room);
    let tail: String = shown.chars().skip(skip).collect();
    canvas.text(
        [x + 10, top + h.saturating_sub(CELL_H) / 2],
        &tail,
        INK,
        clip,
    );
}
