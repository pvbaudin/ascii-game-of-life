//! Shared color palette module (the idea carried over from the lavalamp
//! project). A scheme maps a cell's *age* — and, for dying cells, its fading
//! *trail* — onto a truecolor RGB value. Empty cells are never painted, so the
//! terminal's own background shows through.

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Newborn magenta → mature cyan → ancient blue. Cool and electric.
    Neon,
    /// Fire: newborn white/yellow → orange → deep red embers.
    Ember,
    /// Chlorophyll: bright spring green → deep forest, like a real colony.
    Bio,
    /// Ocean: aqua births cooling to indigo depths.
    Ocean,
    /// Grayscale ramp (also used by --mono, but with hue).
    Mono,
}

impl Scheme {
    pub const ALL: [Scheme; 5] = [
        Scheme::Neon,
        Scheme::Ember,
        Scheme::Bio,
        Scheme::Ocean,
        Scheme::Mono,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Scheme::Neon => "neon",
            Scheme::Ember => "ember",
            Scheme::Bio => "bio",
            Scheme::Ocean => "ocean",
            Scheme::Mono => "mono",
        }
    }

    pub fn from_name(s: &str) -> Option<Scheme> {
        Scheme::ALL.into_iter().find(|c| c.name() == s.to_ascii_lowercase())
    }

    pub fn next(&self) -> Scheme {
        let all = Scheme::ALL;
        let i = all.iter().position(|c| c == self).unwrap_or(0);
        all[(i + 1) % all.len()]
    }

    /// Color for a *live* cell of the given age (1..=MAX_AGE). `t` in 0.0..=1.0
    /// is age normalized (young → old).
    pub fn alive_color(&self, age: u16) -> Rgb {
        // Map age onto a 0..1 curve that moves fast when young, slow when old,
        // so early life color is expressive.
        let t = (age as f32 / 32.0).min(1.0);
        match self {
            Scheme::Neon => lerp3(Rgb(255, 60, 220), Rgb(70, 200, 255), Rgb(60, 90, 255), t),
            Scheme::Ember => lerp3(Rgb(255, 245, 200), Rgb(255, 140, 30), Rgb(170, 20, 10), t),
            Scheme::Bio => lerp3(Rgb(180, 255, 120), Rgb(50, 200, 60), Rgb(10, 90, 40), t),
            Scheme::Ocean => lerp3(Rgb(120, 255, 240), Rgb(40, 140, 210), Rgb(30, 40, 130), t),
            Scheme::Mono => {
                let v = (230.0 - 150.0 * t) as u8;
                Rgb(v, v, v)
            }
        }
    }

    /// Color for a recently-*dead* cell fading out. `trail` is TRAIL_LEN..1.
    /// Returns a dim ember of the scheme; the renderer decides whether trails
    /// are shown at all.
    pub fn trail_color(&self, trail: u8, max: u8) -> Rgb {
        let f = trail as f32 / max as f32; // 1.0 fresh -> ~0 gone
        let base = match self {
            Scheme::Neon => Rgb(90, 40, 120),
            Scheme::Ember => Rgb(120, 40, 20),
            Scheme::Bio => Rgb(40, 80, 40),
            Scheme::Ocean => Rgb(30, 60, 90),
            Scheme::Mono => Rgb(70, 70, 70),
        };
        Rgb(
            (base.0 as f32 * f) as u8,
            (base.1 as f32 * f) as u8,
            (base.2 as f32 * f) as u8,
        )
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    Rgb(lerp(a.0, b.0, t), lerp(a.1, b.1, t), lerp(a.2, b.2, t))
}

/// Two-segment gradient a → mid → b.
fn lerp3(a: Rgb, mid: Rgb, b: Rgb, t: f32) -> Rgb {
    if t < 0.5 {
        lerp_rgb(a, mid, t * 2.0)
    } else {
        lerp_rgb(mid, b, (t - 0.5) * 2.0)
    }
}
