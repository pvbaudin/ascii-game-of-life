//! Headless harnesses gated behind flags, so the engine can be verified without
//! a live terminal. Use these constantly.

use crate::cli::Config;
use crate::life::Life;
use crate::patterns;
use crate::render::Renderer;
use crate::palette::Scheme;
use crate::scenes::{self, Rng, Scene};
use std::time::Instant;

/// Collect the sorted set of live cell coordinates.
fn live_cells(life: &Life) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    for y in 0..life.height {
        for x in 0..life.width {
            if life.is_alive(x, y) {
                v.push((x, y));
            }
        }
    }
    v.sort_unstable();
    v
}

/// `--test`: deterministic invariants. Returns process exit code.
pub fn run_test() -> i32 {
    let mut failures = 0;
    let mut check = |name: &str, ok: bool| {
        println!("[{}] {}", if ok { "PASS" } else { "FAIL" }, name);
        if !ok {
            failures += 1;
        }
    };

    // 1. A block is a still life (unchanged forever).
    {
        let mut life = Life::new(10, 10, false);
        scenes::stamp(&mut life, &patterns::by_name("block").unwrap(), 4, 4);
        let before = live_cells(&life);
        for _ in 0..10 {
            life.step();
        }
        check("a block is still (stable for 10 gens)", live_cells(&life) == before);
    }

    // 2. A blinker oscillates with period 2.
    {
        let mut life = Life::new(10, 10, false);
        scenes::stamp(&mut life, &patterns::by_name("blinker").unwrap(), 4, 4);
        let g0 = live_cells(&life);
        life.step();
        let g1 = live_cells(&life);
        life.step();
        let g2 = live_cells(&life);
        check("a blinker oscillates with period 2", g1 != g0 && g2 == g0);
    }

    // 3. A glider returns to its shape shifted by (1,1) after 4 gens.
    {
        let mut life = Life::new(30, 30, false);
        scenes::stamp(&mut life, &patterns::by_name("glider").unwrap(), 5, 5);
        let g0 = live_cells(&life);
        for _ in 0..4 {
            life.step();
        }
        let g4 = live_cells(&life);
        let shifted: Vec<(usize, usize)> = g0.iter().map(|&(x, y)| (x + 1, y + 1)).collect();
        check("a glider translates by (1,1) after 4 gens", g4 == shifted);
    }

    // 4. Toad has period 2.
    {
        let mut life = Life::new(12, 12, false);
        scenes::stamp(&mut life, &patterns::by_name("toad").unwrap(), 4, 4);
        let g0 = live_cells(&life);
        life.step();
        let g1 = live_cells(&life);
        life.step();
        check("a toad oscillates with period 2", g1 != g0 && live_cells(&life) == g0);
    }

    // 5. Pentadecathlon has period 15.
    {
        let mut life = Life::new(30, 30, false);
        scenes::stamp(&mut life, &patterns::by_name("pentadecathlon").unwrap(), 8, 12);
        let g0 = live_cells(&life);
        let mut returned_at = 0;
        for g in 1..=15 {
            life.step();
            if live_cells(&life) == g0 {
                returned_at = g;
                break;
            }
        }
        check("a pentadecathlon has period 15", returned_at == 15);
    }

    // 6. Gosper gun grows population (emits gliders) over 200 gens.
    {
        let mut life = Life::new(120, 60, false);
        scenes::stamp(&mut life, &patterns::by_name("gosper-glider-gun").unwrap(), 2, 2);
        let p0 = life.population();
        for _ in 0..200 {
            life.step();
        }
        check("a Gosper gun keeps emitting (population grows)", life.population() > p0);
    }

    // 7. RLE round-trip: parsed glider has 5 cells.
    {
        let g = patterns::by_name("glider").unwrap();
        check("RLE glider parses to 5 live cells", g.cells.len() == 5);
    }

    println!("\n{} check(s) failed", failures);
    if failures == 0 {
        0
    } else {
        1
    }
}

/// `--diag N`: population and per-frame change over N generations.
pub fn run_diag(cfg: &Config, gens: u64) -> i32 {
    let mut life = Life::new(160, 80, cfg.wrap);
    let mut rng = Rng::new(cfg.seed);
    scenes::build(cfg.scene, &mut life, &mut rng, cfg.density);
    println!(
        "# scene={} seed={} wrap={} size={}x{}",
        cfg.scene.name(),
        cfg.seed,
        cfg.wrap,
        life.width,
        life.height
    );
    println!("gen\tpopulation\tchanged");
    println!("{}\t{}\t{}", 0, life.population(), 0);
    for g in 1..=gens {
        let changed = life.step();
        println!("{}\t{}\t{}", g, life.population(), changed);
    }
    0
}

/// `--bench`: per-frame update and render cost across grid sizes.
pub fn run_bench(cfg: &Config) -> i32 {
    let sizes = [(80usize, 24usize), (160, 48), (240, 72), (320, 100)];
    let renderer = Renderer {
        braille: cfg.braille,
        mono: cfg.mono,
        show_trails: cfg.trails,
    };
    println!(
        "# renderer={} wrap={}",
        if cfg.braille { "braille" } else { "glyph" },
        cfg.wrap
    );
    println!("cols x rows\tcells\tupdate ms\trender ms\tframe ms\tmax fps\t~CPU% @ {} fps", cfg.fps);
    for (cols, rows) in sizes {
        let (ww, wh) = renderer.world_size(cols, rows);
        let mut life = Life::new(ww, wh, cfg.wrap);
        let mut rng = Rng::new(cfg.seed);
        scenes::build(Scene::Soup, &mut life, &mut rng, 0.3);
        let mut buf = String::new();

        // Warm up.
        for _ in 0..5 {
            life.step();
            renderer.render(&life, cfg.scheme, cols, rows, None, &mut buf);
        }

        let iters = 60;
        let t = Instant::now();
        for _ in 0..iters {
            life.step();
        }
        let update_ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;

        let t = Instant::now();
        for _ in 0..iters {
            renderer.render(&life, cfg.scheme, cols, rows, None, &mut buf);
        }
        let render_ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;

        let frame_ms = update_ms + render_ms;
        let max_fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { f64::INFINITY };
        let cpu = (frame_ms * cfg.fps / 1000.0 * 100.0).min(100.0);
        println!(
            "{}x{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.0}\t{:.1}%",
            cols, rows, ww * wh, update_ms, render_ms, frame_ms, max_fps, cpu
        );
    }
    let _ = Scheme::Neon;
    0
}

/// `--snapshot [gens]`: build the scene, step it `gens` times, and print a
/// single rendered frame (real truecolor ANSI) to stdout, then exit. No live
/// terminal needed — handy for eyeballing a scene or piping to a file.
pub fn run_snapshot(cfg: &Config, gens: u64) -> i32 {
    // Prefer explicit COLUMNS/LINES (handy for reproducible snapshots), then
    // the real terminal size, else a sensible default.
    let env_dim = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
    let (cols, rows) = match (env_dim("COLUMNS"), env_dim("LINES")) {
        (Some(c), Some(r)) => (c, r),
        _ => crossterm::terminal::size()
            .map(|(c, r)| (c as usize, r as usize))
            .unwrap_or((80, 24)),
    };
    let cols = cols.max(1);
    let rows = rows.max(1);
    let renderer = Renderer {
        braille: cfg.braille,
        mono: cfg.mono,
        show_trails: cfg.trails,
    };
    let (ww, wh) = renderer.world_size(cols, rows);
    let mut life = Life::new(ww, wh, cfg.wrap);
    let mut rng = Rng::new(cfg.seed);
    let labels = scenes::build(cfg.scene, &mut life, &mut rng, cfg.density);
    for _ in 0..gens {
        life.step();
    }
    let overlay = crate::build_overlay(&labels, cols, rows, renderer.braille);
    let ov = if overlay.is_empty() { None } else { Some(overlay.as_slice()) };
    let mut frame = String::new();
    renderer.render_with(&life, cfg.scheme, cols, rows, None, ov, &mut frame);
    // Replace the cursor-home lead so it prints inline, and end with a newline.
    let frame = frame.replacen("\x1b[H", "", 1);
    println!("{frame}");
    0
}
