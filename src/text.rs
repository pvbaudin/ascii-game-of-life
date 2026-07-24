//! Reactive text modes, carried over from the lavalamp project's text plumbing.
//!
//! 1. **Reveal**: a text source is laid across the grid and living cells reveal
//!    the character beneath them (colored by age); the colony scans/eats through
//!    the text as it grows and moves. Files scroll slowly and loop; stdin is a
//!    bottom-anchored live tail.
//! 2. **Seed**: incoming text drives the simulation — each word is stamped into
//!    the world as live cells (a glider for short words, letters in a tiny font
//!    for longer ones) which then evolve and dissolve under the rules.
//!
//! Robustness lessons baked in (see comments): read stdin as raw bytes and
//! lossy-decode; keep only the final revision after a carriage return; strip
//! ANSI escapes and all control characters; build streaming scrollback
//! incrementally and ease toward the newest line.

use crate::cli::{Config, TextMode};
use crate::font;
use crate::life::Life;
use crate::patterns;
use crate::scenes::{self, Rng};
use crate::tty;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, Receiver};

pub struct TextEngine {
    enabled: bool,
    mode: TextMode,
    speed: f32,
    streaming: bool,
    lines: Vec<String>,
    rx: Option<Receiver<String>>,
    scroll: f32,
    reveal_buf: Vec<char>,
    reveal_dims: (usize, usize),
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
            lines: Vec::new(),
            rx: None,
            scroll: 0.0,
            reveal_buf: Vec::new(),
            reveal_dims: (0, 0),
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
            eng.lines = load_file(src);
            if eng.lines.is_empty() {
                eng.lines.push(String::from("(empty text source)"));
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
                        // drop trailing newline byte(s)
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

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.reveal_dims = (cols, rows);
        self.reveal_buf = vec![' '; cols.max(1) * rows.max(1)];
    }

    /// Called once per generation before `life.step()`. Drains streamed input,
    /// advances scrolling, and (in seed mode) stamps words into the world.
    pub fn pre_step(&mut self, life: &mut Life, cols: usize, rows: usize, _cfg: &Config) {
        if !self.enabled {
            return;
        }
        self.gen += 1;

        // Drain any newly arrived lines.
        if let Some(rx) = &self.rx {
            while let Ok(line) = rx.try_recv() {
                if self.mode == TextMode::Seed {
                    for w in line.split_whitespace() {
                        self.seed_queue.push_back(w.to_string());
                    }
                } else {
                    self.lines.push(line);
                    // Cap scrollback so memory stays bounded.
                    if self.lines.len() > 5000 {
                        let drop = self.lines.len() - 5000;
                        self.lines.drain(0..drop);
                        self.scroll = (self.scroll - drop as f32).max(0.0);
                    }
                }
            }
        }

        match self.mode {
            TextMode::Reveal => self.advance_scroll(rows),
            TextMode::Seed => self.inject_seed(life, cols, rows),
        }
    }

    fn advance_scroll(&mut self, rows: usize) {
        if self.streaming {
            // Bottom-anchor a live feed like a log tail; ease toward newest.
            let target = self.lines.len().saturating_sub(rows) as f32;
            self.scroll += (target - self.scroll) * 0.15;
        } else if !self.lines.is_empty() {
            // Scroll slowly and loop back to the beginning at the end.
            self.scroll += 0.12 * self.speed;
            let n = self.lines.len() as f32;
            if self.scroll >= n {
                self.scroll -= n;
            }
        }
    }

    fn inject_seed(&mut self, life: &mut Life, cols: usize, rows: usize) {
        // For a file, feed words in gradually so the field stays lively.
        if !self.streaming && self.seed_queue.is_empty() && !self.lines.is_empty() {
            if self.gen % 12 == 0 {
                let line = &self.lines[self.file_cursor % self.lines.len()];
                for w in line.split_whitespace() {
                    self.seed_queue.push_back(w.to_string());
                }
                self.file_cursor += 1;
            }
        }
        // Stamp a couple of queued words per generation.
        for _ in 0..2 {
            let Some(word) = self.seed_queue.pop_front() else { break };
            let clean: String = word.chars().filter(|c| !c.is_whitespace()).collect();
            if clean.is_empty() {
                continue;
            }
            if clean.chars().count() <= 3 {
                // Short word → launch a glider.
                if let Some(g) = patterns::by_name("glider") {
                    let ox = self.rng.below(life.width.saturating_sub(g.width + 1).max(1)) as isize;
                    let oy = self.rng.below(life.height.saturating_sub(g.height + 1).max(1)) as isize;
                    scenes::stamp(life, &g, ox, oy);
                }
            } else {
                // Longer word → stamp its letters in the tiny font.
                let pat = font::word_pattern(&clean);
                if pat.width < cols && pat.height < rows {
                    let ox = self.rng.below(life.width.saturating_sub(pat.width + 1).max(1)) as isize;
                    let oy = self.rng.below(life.height.saturating_sub(pat.height + 1).max(1)) as isize;
                    scenes::stamp(life, &pat, ox, oy);
                }
            }
        }
    }

    /// Build (or reuse) the `cols*rows` reveal grid for the current scroll.
    pub fn reveal_layer(&mut self, cols: usize, rows: usize) -> &[char] {
        if self.reveal_dims != (cols, rows) || self.reveal_buf.len() != cols * rows {
            self.reveal_dims = (cols, rows);
            self.reveal_buf = vec![' '; cols.max(1) * rows.max(1)];
        }
        for c in self.reveal_buf.iter_mut() {
            *c = ' ';
        }
        if self.lines.is_empty() {
            return &self.reveal_buf;
        }
        let n = self.lines.len();
        let top = self.scroll.floor().max(0.0) as usize;
        for y in 0..rows {
            let li = if self.streaming {
                let idx = top + y;
                if idx >= n {
                    continue;
                }
                idx
            } else {
                (top + y) % n
            };
            let line = &self.lines[li];
            for (x, ch) in line.chars().take(cols).enumerate() {
                // Never emit a control character into the frame.
                let ch = if (ch as u32) < 0x20 || ch == '\u{7f}' { ' ' } else { ch };
                self.reveal_buf[y * cols + x] = ch;
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
            // ESC — skip the escape sequence.
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
                    // OSC — until BEL or ESC.
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
        // Strip all other control characters; keep printable bytes.
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
        .filter(|&c| c == ' ' || (!c.is_control()))
        .collect()
}
