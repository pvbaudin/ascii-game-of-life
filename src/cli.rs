//! Hand-rolled argument parser. Supports `--key value`, `--key=value`, and bare
//! boolean flags. Provides a real `--help` and `--version`.

use crate::palette::Scheme;
use crate::scenes::Scene;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextMode {
    /// Living cells reveal the character beneath them.
    Reveal,
    /// Incoming text is stamped into the world as live cells.
    Seed,
}

pub struct Config {
    pub fps: f64,
    pub scheme: Scheme,
    pub seed: u64,
    pub wrap: bool,
    pub scene: Scene,
    pub density: f32,
    pub mono: bool,
    pub braille: bool,
    pub trails: bool,
    pub text: Option<String>,
    pub text_speed: f32,
    pub text_mode: TextMode,
    // Headless harnesses.
    pub test: bool,
    pub diag: Option<u64>,
    pub bench: bool,
    pub snapshot: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            fps: 15.0,
            scheme: Scheme::Neon,
            seed: default_seed(),
            wrap: true,
            scene: Scene::Soup,
            density: 0.28,
            mono: false,
            braille: true,
            trails: true,
            text: None,
            text_speed: 1.0,
            text_mode: TextMode::Reveal,
            test: false,
            diag: None,
            bench: false,
            snapshot: None,
        }
    }
}

fn default_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678)
}

pub enum ParseOutcome {
    Run(Box<Config>),
    Help,
    Version,
    Error(String),
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> ParseOutcome {
    let mut cfg = Config::default();
    let mut it = args.into_iter().peekable();

    // Helper to fetch a value for `--key value` or from `inline` (`--key=value`).
    macro_rules! need {
        ($inline:expr, $it:expr, $key:expr) => {
            match $inline {
                Some(v) => v,
                None => match $it.next() {
                    Some(v) => v,
                    None => return ParseOutcome::Error(format!("missing value for {}", $key)),
                },
            }
        };
    }

    while let Some(arg) = it.next() {
        if !arg.starts_with("--") {
            return ParseOutcome::Error(format!("unexpected argument: {arg}"));
        }
        let body = &arg[2..];
        let (key, inline) = match body.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (body, None),
        };

        match key {
            "help" | "h" => return ParseOutcome::Help,
            "version" | "V" => return ParseOutcome::Version,
            "speed" | "fps" => {
                let v = need!(inline, it, key);
                match v.parse::<f64>() {
                    Ok(f) if f > 0.0 => cfg.fps = f,
                    _ => return ParseOutcome::Error(format!("invalid {key}: {v}")),
                }
            }
            "scheme" => {
                let v = need!(inline, it, key);
                match Scheme::from_name(&v) {
                    Some(s) => cfg.scheme = s,
                    None => return ParseOutcome::Error(format!("unknown scheme: {v}")),
                }
            }
            "seed" => {
                let v = need!(inline, it, key);
                match v.parse::<u64>() {
                    Ok(s) => cfg.seed = s,
                    Err(_) => return ParseOutcome::Error(format!("invalid seed: {v}")),
                }
            }
            "wrap" => {
                let v = need!(inline, it, key);
                cfg.wrap = matches!(v.as_str(), "on" | "true" | "1" | "yes");
            }
            "scene" => {
                let v = need!(inline, it, key);
                match Scene::from_name(&v) {
                    Some(s) => cfg.scene = s,
                    None => return ParseOutcome::Error(format!("unknown scene: {v}")),
                }
            }
            "density" => {
                let v = need!(inline, it, key);
                match v.parse::<f32>() {
                    Ok(d) if (0.0..=1.0).contains(&d) => cfg.density = d,
                    _ => return ParseOutcome::Error(format!("invalid density: {v}")),
                }
            }
            "mono" => cfg.mono = inline.map(|v| truthy(&v)).unwrap_or(true),
            "braille" => cfg.braille = inline.map(|v| truthy(&v)).unwrap_or(true),
            "glyphs" | "glyph" => cfg.braille = false,
            "trails" => cfg.trails = inline.map(|v| truthy(&v)).unwrap_or(true),
            "no-trails" => cfg.trails = false,
            "text" => {
                let v = need!(inline, it, key);
                cfg.text = Some(v);
            }
            "text-speed" => {
                let v = need!(inline, it, key);
                match v.parse::<f32>() {
                    Ok(f) if f > 0.0 => cfg.text_speed = f,
                    _ => return ParseOutcome::Error(format!("invalid text-speed: {v}")),
                }
            }
            "text-mode" => {
                let v = need!(inline, it, key);
                match v.as_str() {
                    "reveal" => cfg.text_mode = TextMode::Reveal,
                    "seed" => cfg.text_mode = TextMode::Seed,
                    _ => return ParseOutcome::Error(format!("unknown text-mode: {v}")),
                }
            }
            "test" => cfg.test = true,
            "diag" => {
                let n = inline
                    .or_else(|| it.peek().filter(|s| !s.starts_with("--")).cloned().inspect(|_| { it.next(); }))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(200);
                cfg.diag = Some(n);
            }
            "bench" => cfg.bench = true,
            "snapshot" => {
                let n = inline
                    .or_else(|| it.peek().filter(|s| !s.starts_with("--")).cloned().inspect(|_| { it.next(); }))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                cfg.snapshot = Some(n);
            }
            other => return ParseOutcome::Error(format!("unknown flag: --{other}")),
        }
    }

    // Reveal mode implies glyph resolution (one char per cell).
    if cfg.text.is_some() && cfg.text_mode == TextMode::Reveal {
        cfg.braille = false;
    }

    ParseOutcome::Run(Box::new(cfg))
}

fn truthy(v: &str) -> bool {
    matches!(v, "on" | "true" | "1" | "yes")
}

pub fn help() -> String {
    format!(
        "life {VERSION} — ASCII-art Conway's Game of Life\n\
\n\
USAGE:\n\
    life [OPTIONS]\n\
\n\
OPTIONS:\n\
    --fps, --speed <N>      Generations per second (default 15)\n\
    --scheme <NAME>         Color scheme: neon, ember, bio, ocean, mono\n\
    --seed <N>              PRNG seed for reproducible soups\n\
    --wrap <on|off>         Toroidal edges (on) or fixed dead border (off)\n\
    --scene <NAME>          Opening scene: garden, soup, methuselah\n\
    --density <0..1>        Soup fill fraction (default 0.28)\n\
    --mono                  Monochrome (grayscale by age)\n\
    --braille               Braille sub-pixel renderer (default; 8x res)\n\
    --glyphs                Plain glyph renderer (·:+oO@#)\n\
    --no-trails             Disable fading trails of recently-dead cells\n\
    --text <FILE|->         Text source ('-' = stdin) for text modes\n\
    --text-mode <MODE>      reveal (cells reveal text) or seed (text -> life)\n\
    --text-speed <N>        Text scroll/inject speed multiplier (default 1)\n\
\n\
HEADLESS HARNESSES (no terminal needed):\n\
    --test                  Run invariant checks (blinker/block/glider) & exit\n\
    --diag [N]              Print population/change over N gens & exit\n\
    --bench                 Benchmark update+render across grid sizes & exit\n\
    --snapshot [N]          Print one rendered frame after N gens & exit\n\
\n\
    --help                  Show this help\n\
    --version               Show version\n\
\n\
INTERACTIVE KEYS:\n\
    q / Esc / Ctrl-C  quit      space  pause      .  single-step\n\
    + / -  faster / slower      c  cycle scheme    r  reseed / next scene\n\
    m  toggle mono              b  toggle braille\n"
    )
}
