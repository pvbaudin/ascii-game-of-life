//! Frame renderer. Builds each frame into a single `String` of raw ANSI
//! truecolor and hands it back to be written once. A `SetForegroundColor`
//! escape is only emitted when the color actually changes from the previous
//! cell (a big perf win). Empty cells are written as spaces with no background,
//! so the terminal's own background shows through.
//!
//! Two looks:
//!   - **braille** (`U+2800`–`U+28FF`): each character packs a 2×4 grid of
//!     sub-pixels, giving 8× the resolution. The world is sized to (2·cols,
//!     4·rows) and each glyph aggregates its 8 sub-cells.
//!   - **glyph**: one world cell per character, drawn from a density ramp by
//!     age. This resolution is also what the reveal text-mode uses.

use crate::life::{Life, TRAIL_LEN};
use crate::palette::{Rgb, Scheme};
use std::fmt::Write as _;

/// Density ramp for glyph mode, indexed young → old.
const RAMP: &[char] = &['·', ':', '+', 'o', 'O', '@', '#'];

pub struct Renderer {
    pub braille: bool,
    pub mono: bool,
    pub show_trails: bool,
}

impl Renderer {
    /// World dimensions this renderer wants for the given terminal size.
    pub fn world_size(&self, cols: usize, rows: usize) -> (usize, usize) {
        if self.braille {
            (cols * 2, rows * 4)
        } else {
            (cols, rows)
        }
    }

    /// Render a frame. `cols`/`rows` are the terminal size. `reveal`, when
    /// present, is a `cols*rows` grid of characters (glyph mode only): a live
    /// cell reveals `reveal[i]` instead of a density glyph. `overlay`, when
    /// present, is a `cols*rows` grid of dim label characters drawn on empty
    /// cells (used for the garden scene's labels).
    pub fn render(
        &self,
        life: &Life,
        scheme: Scheme,
        cols: usize,
        rows: usize,
        reveal: Option<&[char]>,
        out: &mut String,
    ) {
        self.render_with(life, scheme, cols, rows, reveal, None, out);
    }

    pub fn render_with(
        &self,
        life: &Life,
        scheme: Scheme,
        cols: usize,
        rows: usize,
        reveal: Option<&[char]>,
        overlay: Option<&[char]>,
        out: &mut String,
    ) {
        out.clear();
        out.push_str("\x1b[H"); // cursor home
        let mut last: Option<Rgb> = None;

        if self.braille {
            self.render_braille(life, scheme, cols, rows, overlay, &mut last, out);
        } else {
            self.render_glyph(life, scheme, cols, rows, reveal, overlay, &mut last, out);
        }
        out.push_str("\x1b[0m");
    }

    fn render_glyph(
        &self,
        life: &Life,
        scheme: Scheme,
        cols: usize,
        rows: usize,
        reveal: Option<&[char]>,
        overlay: Option<&[char]>,
        last: &mut Option<Rgb>,
        out: &mut String,
    ) {
        for y in 0..rows {
            for x in 0..cols {
                if x >= life.width || y >= life.height {
                    push_overlay_or_empty(out, last, overlay, cols, x, y);
                    continue;
                }
                let i = life.idx(x, y);
                if life.is_alive(x, y) {
                    let age = life.age[i];
                    let color = if self.mono {
                        Scheme::Mono.alive_color(age)
                    } else {
                        scheme.alive_color(age)
                    };
                    let ch = match reveal {
                        Some(r) if r[y * cols + x] != ' ' => r[y * cols + x],
                        _ => ramp_char(age),
                    };
                    emit(out, last, color, ch);
                } else if self.show_trails && life.trail[i] > 0 {
                    let color = if self.mono {
                        Scheme::Mono.trail_color(life.trail[i], TRAIL_LEN)
                    } else {
                        scheme.trail_color(life.trail[i], TRAIL_LEN)
                    };
                    emit(out, last, color, '·');
                } else {
                    push_overlay_or_empty(out, last, overlay, cols, x, y);
                }
            }
            if y + 1 < rows {
                out.push_str("\r\n");
            }
        }
    }

    fn render_braille(
        &self,
        life: &Life,
        scheme: Scheme,
        cols: usize,
        rows: usize,
        overlay: Option<&[char]>,
        last: &mut Option<Rgb>,
        out: &mut String,
    ) {
        // Braille dot bit for sub-cell (dx in 0..2, dy in 0..4).
        const BITS: [[u8; 4]; 2] = [
            [0x01, 0x02, 0x04, 0x40], // left column: dots 1,2,3,7
            [0x08, 0x10, 0x20, 0x80], // right column: dots 4,5,6,8
        ];
        for cy in 0..rows {
            for cx in 0..cols {
                let mut bits: u8 = 0;
                let mut max_age: u16 = 0;
                let mut max_trail: u8 = 0;
                for dx in 0..2usize {
                    for dy in 0..4usize {
                        let wx = cx * 2 + dx;
                        let wy = cy * 4 + dy;
                        if wx >= life.width || wy >= life.height {
                            continue;
                        }
                        let i = life.idx(wx, wy);
                        if life.is_alive(wx, wy) {
                            bits |= BITS[dx][dy];
                            max_age = max_age.max(life.age[i]);
                        } else {
                            max_trail = max_trail.max(life.trail[i]);
                        }
                    }
                }
                if bits != 0 {
                    let color = if self.mono {
                        Scheme::Mono.alive_color(max_age)
                    } else {
                        scheme.alive_color(max_age)
                    };
                    let ch = char::from_u32(0x2800 + bits as u32).unwrap_or('?');
                    emit(out, last, color, ch);
                } else if self.show_trails && max_trail > 0 {
                    let color = if self.mono {
                        Scheme::Mono.trail_color(max_trail, TRAIL_LEN)
                    } else {
                        scheme.trail_color(max_trail, TRAIL_LEN)
                    };
                    // A faint braille dot for the fading trail.
                    emit(out, last, color, '\u{2801}');
                } else {
                    push_overlay_or_empty(out, last, overlay, cols, cx, cy);
                }
            }
            if cy + 1 < rows {
                out.push_str("\r\n");
            }
        }
    }
}

fn ramp_char(age: u16) -> char {
    let t = (age as f32 / 20.0).min(1.0);
    let i = (t * (RAMP.len() - 1) as f32).round() as usize;
    RAMP[i.min(RAMP.len() - 1)]
}

#[inline]
fn emit(out: &mut String, last: &mut Option<Rgb>, color: Rgb, ch: char) {
    let changed = match last {
        Some(p) => p.0 != color.0 || p.1 != color.1 || p.2 != color.2,
        None => true,
    };
    if changed {
        let _ = write!(out, "\x1b[38;2;{};{};{}m", color.0, color.1, color.2);
        *last = Some(color);
    }
    out.push(ch);
}

#[inline]
fn push_empty(out: &mut String, _last: &mut Option<Rgb>) {
    // Foreground color is irrelevant for a blank; leave it as-is to avoid an
    // escape. The space paints nothing, so the terminal background shows.
    out.push(' ');
}

/// Dim gray used for garden labels.
const LABEL: Rgb = Rgb(96, 96, 110);

#[inline]
fn push_overlay_or_empty(
    out: &mut String,
    last: &mut Option<Rgb>,
    overlay: Option<&[char]>,
    cols: usize,
    x: usize,
    y: usize,
) {
    if let Some(o) = overlay {
        let ch = o[y * cols + x];
        if ch != ' ' {
            emit(out, last, LABEL, ch);
            return;
        }
    }
    push_empty(out, last);
}
