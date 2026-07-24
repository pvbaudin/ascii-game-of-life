//! Reactive text modes, carried over from the lavalamp project's text plumbing.
//!
//! 1. **Reveal**: a text source is laid across the grid and living cells reveal
//!    the character beneath them (colored by age); the colony scans/eats through
//!    the text as it grows and moves. The source is word-wrapped to the terminal
//!    width into a tall canvas; the visible window scrolls slowly *down* through
//!    it and **loops back to the top** at the end (just like lavalamp). A live
//!    stdin stream instead flows as a bottom-anchored tail.
//! 2. **Seed**: incoming text drives the simulation — each word is stamped into
//!    the world as live cells (a glider for short words, letters in a tiny font
//!    for longer ones) which then evolve and dissolve under the rules.
//!
//! Robustness lessons baked in (see comments): read stdin as raw bytes and
//! lossy-decode; keep only the final revision after a carriage return; strip
//! ANSI escapes and all control characters; build streaming scrollback
//! incrementally (never re-wrap existing rows) and ease toward the newest line.

use crate::cli::{Config, TextMode};
use crate::font;
use crate::life::Life;
use crate::patterns;
use crate::scenes::{self, Rng};
use crate::tty;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

/// Base reveal scroll rate in canvas-rows per second; `--text-speed` scales it.
const SCROLL_ROWS_PER_SEC: f32 = 1.1;
/// How quickly a live stream eases toward its newest (bottom) line.
const STREAM_SCROLL_EASE: f32 = 3.5;

pub struct TextEngine {
    enabled: bool,
    mode: TextMode,
    speed: f32,
    streaming: bool,
    /// Raw source lines (file/PDF), kept so the canvas can be re-wrapped on
    /// resize without re-reading the file. Empty in stream mode.
    source: Vec<String>,
    /// Word-wrapped rows at the current width; the scrolling window reads this.
    canvas: Vec<Vec<char>>,
    canvas_width: usize,
    rx: Option<Receiver<String>>,
    scroll: f32,
    scroll_target: f32,
    last_scroll: Instant,
    width: usize,
    reveal_buf: Vec<char>,
    seed_queue: VecDeque<String>,
    file_cursor: usize,
    gen: u64,
    rng: Rng,
}

impl TextEngine {
    pub fn new(cfg: &Config) -> Self {
        let mut eng = TextEngine {
            enabled: cfg.text.is_some(),
            mode: cfg.text_mode,
            speed: cfg.text_speed,
            streaming: false,
            source: Vec::new(),
            canvas: Vec::new(),
            canvas_width: 0,
            rx: None,
            scroll: 0.0,
            scroll_target: 0.0,
            last_scroll: Instant::now(),
            width: 0,
            reveal_buf: Vec::new(),
            seed_queue: VecDeque::new(),
            file_cursor: 0,
            gen: 0,
            rng: Rng::new(cfg.seed ^ 0xA5A5_1234),
        };

        let Some(src) = cfg.text.as_deref() else {
            return eng;
        };

        if src == "-" {
            eng.streaming = true;
            eng.start_stream();
        } else {
            eng.source = load_file(src);
            if eng.source.is_empty() {
                eng.source.push(String::from("(empty text source)"));
            }
        }
        eng
    }

    /// Spawn a background thread reading the piped input as raw bytes, applying
    /// the robustness rules, and sending finalized lines over a channel.
    fn start_stream(&mut self) {
        // Redirect fd 0 to a real terminal for crossterm *before* it inits, and
        // read the piped data from the saved fd.
        let reader: Option<Box<dyn Read + Send>> = match tty::redirect_stdin_if_piped() {
            Some(f) => Some(Box::new(f)),
            None => {
                // stdin is a real tty (no piped data) → nothing to stream.
                if unsafe { libc::isatty(0) } == 1 {
                    None
                } else {
                    // A pipe with no terminal available for keys; still read it.
                    Some(Box::new(std::io::stdin()))
                }
            }
        };
        let Some(reader) = reader else { return };

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let mut br = BufReader::new(reader);
            let mut raw: Vec<u8> = Vec::new();
            loop {
                raw.clear();
                match br.read_until(b'\n', &mut raw) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        while matches!(raw.last(), Some(b'\n')) {
                            raw.pop();
                        }
                        let line = clean_line(&raw);
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    pub fn resize(&mut self, cols: usize, _rows: usize) {
        self.width = cols.max(1);
        if self.streaming {
            // Existing rows were wrapped to the old width; start fresh so new
            // lines refill at the new width (don't re-wrap/jitter old content).
            self.canvas.clear();
            self.scroll = 0.0;
            self.scroll_target = 0.0;
            self.canvas_width = self.width;
        } else {
            self.rebuild_canvas(self.width);
        }
    }

    /// Word-wrap `line` to width `w`, appending its rows to `canvas`. Always adds
    /// at least one row (may be blank → a separator). Over-long words hard-break.
    fn wrap_line(line: &str, w: usize, canvas: &mut Vec<Vec<char>>) {
        let mut row: Vec<char> = Vec::new();
        for word in line.split_whitespace() {
            let wl = word.chars().count();
            if !row.is_empty() && row.len() + 1 + wl > w {
                canvas.push(std::mem::take(&mut row));
            }
            if wl > w {
                for ch in word.chars() {
                    if row.len() >= w {
                        canvas.push(std::mem::take(&mut row));
                    }
                    row.push(ch);
                }
            } else {
                if !row.is_empty() {
                    row.push(' ');
                }
                row.extend(word.chars());
            }
        }
        canvas.push(row);
    }

    /// Rebuild the wrapped canvas from `source` at width `w` (file/PDF mode).
    fn rebuild_canvas(&mut self, w: usize) {
        let w = w.max(1);
        self.canvas_width = w;
        self.canvas.clear();
        for raw in &self.source {
            Self::wrap_line(raw, w, &mut self.canvas);
        }
        // Drop a trailing blank so there's no long gap at the wrap/loop point.
        if self.canvas.last().map(|r| r.is_empty()).unwrap_or(false) {
            self.canvas.pop();
        }
        if self.scroll >= self.canvas.len() as f32 {
            self.scroll = 0.0;
        }
    }

    /// Drain newly-arrived stream lines, wrapping them into the canvas
    /// incrementally (existing rows are never re-wrapped) and easing the bottom
    /// anchor toward the newest line.
    fn pump_stream(&mut self, rows: usize) {
        let Some(rx) = &self.rx else { return };
        let mut got = false;
        let w = self.width.max(1);
        while let Ok(line) = rx.try_recv() {
            if self.mode == TextMode::Seed {
                for word in line.split_whitespace() {
                    self.seed_queue.push_back(word.to_string());
                }
            } else {
                Self::wrap_line(&line, w, &mut self.canvas);
                got = true;
            }
        }
        if got {
            // Bounded scrollback: a few screenfuls of history.
            let cap = (rows * 6).max(200);
            if self.canvas.len() > cap {
                let drop = self.canvas.len() - cap;
                self.canvas.drain(0..drop);
                self.scroll = (self.scroll - drop as f32).max(0.0);
            }
            self.scroll_target = (self.canvas.len() as f32 - rows as f32).max(0.0);
        }
    }

    /// Called once per generation before `life.step()`. In seed mode, stamps
    /// queued words into the world. (Reveal draining/scrolling happens in
    /// `reveal_layer`, which runs every frame so it's smooth even when paused.)
    pub fn pre_step(&mut self, life: &mut Life, cols: usize, rows: usize, _cfg: &Config) {
        if !self.enabled {
            return;
        }
        self.gen += 1;
        if self.mode == TextMode::Seed {
            self.pump_stream(rows);
            self.inject_seed(life, cols, rows);
        }
    }

    fn inject_seed(&mut self, life: &mut Life, cols: usize, rows: usize) {
        // For a file, feed words in gradually so the field stays lively.
        if !self.streaming && self.seed_queue.is_empty() && !self.source.is_empty() {
            if self.gen % 12 == 0 {
                let line = &self.source[self.file_cursor % self.source.len()];
                for w in line.split_whitespace() {
                    self.seed_queue.push_back(w.to_string());
                }
                self.file_cursor += 1;
            }
        }
        for _ in 0..2 {
            let Some(word) = self.seed_queue.pop_front() else { break };
            let clean: String = word.chars().filter(|c| !c.is_whitespace()).collect();
            if clean.is_empty() {
                continue;
            }
            if clean.chars().count() <= 3 {
                if let Some(g) = patterns::by_name("glider") {
                    let ox = self.rng.below(life.width.saturating_sub(g.width + 1).max(1)) as isize;
                    let oy = self.rng.below(life.height.saturating_sub(g.height + 1).max(1)) as isize;
                    scenes::stamp(life, &g, ox, oy);
                }
            } else {
                let pat = font::word_pattern(&clean);
                if pat.width < cols && pat.height < rows {
                    let ox = self.rng.below(life.width.saturating_sub(pat.width + 1).max(1)) as isize;
                    let oy = self.rng.below(life.height.saturating_sub(pat.height + 1).max(1)) as isize;
                    scenes::stamp(life, &pat, ox, oy);
                }
            }
        }
    }

    /// Advance the reveal scroll by real elapsed time. File mode drifts down at a
    /// steady rate and loops back to the top; a live stream eases toward its
    /// newest line (bottom-anchored tail).
    fn advance_scroll(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_scroll).as_secs_f32().min(0.25);
        self.last_scroll = now;
        if self.canvas.is_empty() {
            return;
        }
        if self.streaming {
            let ease = (STREAM_SCROLL_EASE * dt).min(1.0);
            self.scroll += (self.scroll_target - self.scroll) * ease;
        } else {
            let n = self.canvas.len() as f32;
            self.scroll += dt * SCROLL_ROWS_PER_SEC * self.speed;
            if self.scroll >= n {
                self.scroll -= n; // reached the end → loop back to the start
            }
        }
    }

    /// Build the `cols*rows` reveal grid for the current scroll. Called every
    /// frame; drains the stream, advances the (looping) scroll, then samples the
    /// wrapped canvas so each visible row maps to `floor(scroll)+row`.
    pub fn reveal_layer(&mut self, cols: usize, rows: usize) -> &[char] {
        self.width = cols.max(1);
        if self.reveal_buf.len() != cols * rows {
            self.reveal_buf = vec![' '; cols.max(1) * rows.max(1)];
        }
        // (Re)wrap the file canvas if the width changed.
        if !self.streaming && self.canvas_width != self.width {
            self.rebuild_canvas(self.width);
        }
        self.pump_stream(rows);
        self.advance_scroll();

        for c in self.reveal_buf.iter_mut() {
            *c = ' ';
        }
        let n = self.canvas.len();
        if n == 0 {
            return &self.reveal_buf;
        }
        let base = self.scroll.floor().max(0.0) as usize;
        for y in 0..rows {
            // File mode wraps (loops the source); a stream doesn't repeat.
            let cr = if self.streaming {
                let idx = base + y;
                if idx >= n {
                    continue;
                }
                idx
            } else {
                (base + y) % n
            };
            let src_row = &self.canvas[cr];
            for x in 0..cols.min(src_row.len()) {
                let ch = src_row[x];
                // Never emit a control character (or blank) into the frame.
                if ch != ' ' && (ch as u32) >= 0x20 && ch != '\u{7f}' {
                    self.reveal_buf[y * cols + x] = ch;
                }
            }
        }
        &self.reveal_buf
    }
}

/// Keep only the final revision (after the last `\r`), strip ANSI escape
/// sequences and all control characters, then lossy-decode to UTF-8.
fn clean_line(raw: &[u8]) -> String {
    let tail = match raw.iter().rposition(|&b| b == b'\r') {
        Some(i) => &raw[i + 1..],
        None => raw,
    };
    let mut out: Vec<u8> = Vec::with_capacity(tail.len());
    let mut i = 0;
    while i < tail.len() {
        let b = tail[i];
        if b == 0x1b {
            i += 1;
            match tail.get(i) {
                Some(b'[') => {
                    i += 1;
                    while i < tail.len() && !(0x40..=0x7e).contains(&tail[i]) {
                        i += 1;
                    }
                    if i < tail.len() {
                        i += 1;
                    }
                }
                Some(b']') => {
                    i += 1;
                    while i < tail.len() && tail[i] != 0x07 && tail[i] != 0x1b {
                        i += 1;
                    }
                    if i < tail.len() {
                        i += 1;
                    }
                }
                Some(_) => i += 1,
                None => {}
            }
            continue;
        }
        if b >= 0x20 && b != 0x7f {
            out.push(b);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Load a text file (any encoding, lossy) or extract text from a PDF.
fn load_file(path: &str) -> Vec<String> {
    if path.to_ascii_lowercase().ends_with(".pdf") {
        #[cfg(feature = "pdf")]
        {
            if let Ok(text) = pdf_extract::extract_text(path) {
                return text.lines().map(sanitize).collect();
            }
        }
        return vec![format!("(could not read PDF: {path})")];
    }
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes)
            .lines()
            .map(sanitize)
            .collect(),
        Err(e) => vec![format!("(could not read {path}: {e})")],
    }
}

/// Strip control chars from a decoded string line (for file/PDF text).
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|&c| c == ' ' || !c.is_control())
        .collect()
}
