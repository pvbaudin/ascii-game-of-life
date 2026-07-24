//! ASCII-art Conway's Game of Life — a full-color terminal screensaver.
//!
//! # Edge model
//!
//! The world is a **torus** by default (`--wrap on`): the left edge is stitched
//! to the right and top to bottom, so the colony never hits a wall and the scene
//! feels continuous and endless — ideal for a screensaver. The tradeoff is that
//! showcase patterns designed for an infinite plane behave differently on a
//! torus: a glider gun's stream eventually wraps around and collides with the
//! gun, and spaceships re-enter from the far side. For faithfully watching guns
//! and spaceships, pass `--wrap off`, which makes the border fixed dead space so
//! escaping structures simply sail off and vanish. We keep it simple — the world
//! is exactly the viewport resolution and reflows on resize — rather than
//! maintaining a larger off-screen world with a panning camera; that's a natural
//! future iteration.

mod cli;
mod font;
mod harness;
mod life;
mod palette;
mod patterns;
mod render;
mod rle;
mod scenes;
mod text;
mod tty;

use cli::{Config, ParseOutcome, TextMode};
use life::Life;
use render::Renderer;
use scenes::Rng;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{cursor, terminal, ExecutableCommand};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match cli::parse(args) {
        ParseOutcome::Run(c) => *c,
        ParseOutcome::Help => {
            print!("{}", cli::help());
            return;
        }
        ParseOutcome::Version => {
            println!("life {}", cli::VERSION);
            return;
        }
        ParseOutcome::Error(e) => {
            eprintln!("error: {e}\n\nTry 'life --help'.");
            std::process::exit(2);
        }
    };

    // Headless harnesses never touch the terminal.
    if cfg.test {
        std::process::exit(harness::run_test());
    }
    if let Some(n) = cfg.diag {
        std::process::exit(harness::run_diag(&cfg, n));
    }
    if cfg.bench {
        std::process::exit(harness::run_bench(&cfg));
    }
    if let Some(n) = cfg.snapshot {
        std::process::exit(harness::run_snapshot(&cfg, n));
    }

    if let Err(e) = run(cfg) {
        // Ensure the terminal is sane before reporting.
        let _ = restore_terminal();
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// RAII guard so the terminal is restored on every exit path, including panics.
struct TermGuard;
impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn restore_terminal() -> std::io::Result<()> {
    let mut out = std::io::stdout();
    out.execute(terminal::LeaveAlternateScreen)?;
    out.execute(cursor::Show)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

fn run(mut cfg: Config) -> std::io::Result<()> {
    // If a text source is set and stdin is a pipe, redirect fd 0 to a real tty
    // *before* crossterm initializes, and read the piped data from a saved fd.
    // (See tty.rs — the hard-won crossterm-on-macOS lesson.)
    let mut text_engine = text::TextEngine::new(&cfg);

    // Install a panic hook that restores the terminal first.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));

    terminal::enable_raw_mode()?;
    let mut out = std::io::stdout();
    out.execute(terminal::EnterAlternateScreen)?;
    out.execute(cursor::Hide)?;
    out.execute(terminal::Clear(terminal::ClearType::All))?;
    let _guard = TermGuard;

    let (mut cols, mut rows) = terminal::size().map(|(c, r)| (c as usize, r as usize))?;
    cols = cols.max(1);
    rows = rows.max(1);

    let mut renderer = Renderer {
        braille: cfg.braille,
        mono: cfg.mono,
        show_trails: cfg.trails,
    };
    let (ww, wh) = renderer.world_size(cols, rows);
    let mut life = Life::new(ww, wh, cfg.wrap);
    let mut rng = Rng::new(cfg.seed);
    let mut labels = scenes::build(cfg.scene, &mut life, &mut rng, cfg.density);
    let mut overlay = build_overlay(&labels, cols, rows, renderer.braille);

    let mut frame = String::with_capacity(cols * rows * 4);
    let mut paused = false;
    let mut single_step = false;
    let mut last_gen = Instant::now();

    loop {
        let period = Duration::from_secs_f64(1.0 / cfg.fps.clamp(0.25, 120.0));

        // ---- input ----
        if event::poll(Duration::from_millis(5))? {
            match event::read()? {
                Event::Key(k) => {
                    match handle_key(k, &mut cfg, &mut paused, &mut single_step) {
                        KeyAction::Quit => break,
                        KeyAction::Reseed => {
                            cfg.seed = cfg.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                            cfg.scene = cfg.scene.next();
                            rng = Rng::new(cfg.seed);
                            labels = scenes::build(cfg.scene, &mut life, &mut rng, cfg.density);
                            overlay = build_overlay(&labels, cols, rows, renderer.braille);
                        }
                        KeyAction::ToggleBraille => {
                            renderer.braille = cfg.braille;
                            let (ww, wh) = renderer.world_size(cols, rows);
                            life.resize(ww, wh);
                            rng = Rng::new(cfg.seed);
                            labels = scenes::build(cfg.scene, &mut life, &mut rng, cfg.density);
                            overlay = build_overlay(&labels, cols, rows, renderer.braille);
                            out.execute(terminal::Clear(terminal::ClearType::All))?;
                        }
                        KeyAction::ToggleMono => renderer.mono = cfg.mono,
                        KeyAction::None => {}
                    }
                }
                Event::Resize(c, r) => {
                    cols = (c as usize).max(1);
                    rows = (r as usize).max(1);
                    let (ww, wh) = renderer.world_size(cols, rows);
                    life.resize(ww, wh);
                    text_engine.resize(cols, rows);
                    overlay = build_overlay(&labels, cols, rows, renderer.braille);
                    out.execute(terminal::Clear(terminal::ClearType::All))?;
                }
                _ => {}
            }
        }

        // ---- advance simulation ----
        let due = last_gen.elapsed() >= period;
        if (!paused && due) || single_step {
            text_engine.pre_step(&mut life, cols, rows, &cfg);
            life.step();
            last_gen = Instant::now();
            single_step = false;
        }

        // ---- render ----
        let reveal = if cfg.text.is_some() && cfg.text_mode == TextMode::Reveal {
            Some(text_engine.reveal_layer(cols, rows))
        } else {
            None
        };
        let ov = if overlay.is_empty() { None } else { Some(overlay.as_slice()) };
        renderer.render_with(&life, cfg.scheme, cols, rows, reveal, ov, &mut frame);
        out.write_all(frame.as_bytes())?;
        out.flush()?;

        // Sleep a touch to avoid a busy spin when paused / between gens.
        if paused {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    Ok(())
}

/// Rasterize garden labels into a `cols*rows` overlay grid. Labels are in
/// world-cell coordinates; in braille mode a world cell is a sub-pixel (2×4 per
/// character) so text would be unreadable — we only show labels in glyph mode,
/// where world cells map 1:1 to terminal cells.
fn build_overlay(labels: &[scenes::Label], cols: usize, rows: usize, braille: bool) -> Vec<char> {
    if braille || labels.is_empty() {
        return Vec::new();
    }
    let mut grid = vec![' '; cols * rows];
    for label in labels {
        if label.y >= rows {
            continue;
        }
        for (i, ch) in label.text.chars().enumerate() {
            let x = label.x + i;
            if x >= cols {
                break;
            }
            grid[label.y * cols + x] = ch;
        }
    }
    grid
}

enum KeyAction {
    None,
    Quit,
    Reseed,
    ToggleBraille,
    ToggleMono,
}

fn handle_key(
    k: KeyEvent,
    cfg: &mut Config,
    paused: &mut bool,
    single_step: &mut bool,
) -> KeyAction {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Char('q') | KeyCode::Esc => KeyAction::Quit,
        KeyCode::Char('c') if ctrl => KeyAction::Quit,
        KeyCode::Char(' ') => {
            *paused = !*paused;
            KeyAction::None
        }
        KeyCode::Char('.') => {
            *single_step = true;
            KeyAction::None
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            cfg.fps = (cfg.fps * 1.25).min(120.0);
            KeyAction::None
        }
        KeyCode::Char('-') | KeyCode::Char('_') => {
            cfg.fps = (cfg.fps / 1.25).max(0.25);
            KeyAction::None
        }
        KeyCode::Char('c') => {
            cfg.scheme = cfg.scheme.next();
            KeyAction::None
        }
        KeyCode::Char('r') | KeyCode::Char('n') => KeyAction::Reseed,
        KeyCode::Char('m') => {
            cfg.mono = !cfg.mono;
            KeyAction::ToggleMono
        }
        KeyCode::Char('b') => {
            cfg.braille = !cfg.braille;
            KeyAction::ToggleBraille
        }
        _ => KeyAction::None,
    }
}

// Keep the `Read`/`Write` traits used even if some paths are compiled out.
#[allow(dead_code)]
fn _trait_anchor(mut r: impl Read, mut w: impl Write) {
    let mut b = [0u8; 1];
    let _ = r.read(&mut b);
    let _ = w.write(&b);
}
