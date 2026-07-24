//! Opening scenes and reseeding. Three curated openers so there's fascinating
//! structure from the very first frame:
//!   - **garden**: a tidy grid of the still lifes and oscillators.
//!   - **soup**: random cells at `density`, sprinkled with a Gosper gun and a
//!     few spaceships for constant motion.
//!   - **methuselah**: a single small chaotic seed (R-pentomino / acorn /
//!     diehard), centered, to watch evolve for hundreds of generations.

use crate::life::Life;
use crate::patterns::{self, Category};
use crate::rle::Pattern;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Garden,
    Soup,
    Methuselah,
}

impl Scene {
    pub const ALL: [Scene; 3] = [Scene::Garden, Scene::Soup, Scene::Methuselah];
    pub fn name(&self) -> &'static str {
        match self {
            Scene::Garden => "garden",
            Scene::Soup => "soup",
            Scene::Methuselah => "methuselah",
        }
    }
    pub fn from_name(s: &str) -> Option<Scene> {
        Scene::ALL.into_iter().find(|c| c.name() == s.to_ascii_lowercase())
    }
    pub fn next(&self) -> Scene {
        let i = Scene::ALL.iter().position(|c| c == self).unwrap_or(0);
        Scene::ALL[(i + 1) % Scene::ALL.len()]
    }
}

/// Small, fast, seedable PRNG (SplitMix64) so `--seed` is reproducible.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_add(0x9E3779B97F4A7C15))
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut z = self.0;
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        (self.next_u64() >> 40) as f32 / ((1u64 << 24) as f32) < p
    }
}

/// Stamp a decoded pattern with its top-left at (ox, oy).
pub fn stamp(life: &mut Life, pat: &Pattern, ox: isize, oy: isize) {
    for &(x, y) in &pat.cells {
        life.set(ox + x as isize, oy + y as isize, true);
    }
}

/// A text label for the garden scene, in world-cell coordinates.
pub struct Label {
    pub x: usize,
    pub y: usize,
    pub text: String,
}

pub fn build(scene: Scene, life: &mut Life, rng: &mut Rng, density: f32) -> Vec<Label> {
    life.clear();
    match scene {
        Scene::Garden => garden(life),
        Scene::Soup => {
            soup(life, rng, density);
            Vec::new()
        }
        Scene::Methuselah => {
            methuselah(life, rng);
            Vec::new()
        }
    }
}

fn garden(life: &mut Life) -> Vec<Label> {
    // Collect still lifes + oscillators (with names), sorted by height so
    // similar sizes shelf-pack tightly rather than being dominated by the pulsar.
    let mut specimens: Vec<(&'static str, Pattern)> = Vec::new();
    for p in patterns::in_category(Category::StillLife) {
        specimens.push((p.name, p.pattern()));
    }
    for p in patterns::in_category(Category::Oscillator) {
        specimens.push((p.name, p.pattern()));
    }
    specimens.sort_by_key(|(_, p)| p.height);

    // Shelf-packing: flow left→right, wrap to a new shelf when the row is full.
    // Each item reserves space for the pattern plus a label row beneath it.
    let mut labels = Vec::new();
    let margin_x = 3usize;
    let mut x = 1usize;
    let mut y = 1usize;
    let mut shelf_h = 0usize;

    for (name, pat) in &specimens {
        let box_w = pat.width.max(name.len()) + margin_x;
        if x + box_w >= life.width {
            // New shelf.
            x = 1;
            y += shelf_h + 2;
            shelf_h = 0;
        }
        let item_h = pat.height + 1; // +1 for the label row
        if y + item_h >= life.height {
            break;
        }
        stamp(life, pat, x as isize, y as isize);
        labels.push(Label {
            x,
            y: y + pat.height,
            text: name.to_string(),
        });
        shelf_h = shelf_h.max(item_h);
        x += box_w + 1;
    }
    labels
}

fn soup(life: &mut Life, rng: &mut Rng, density: f32) {
    // Random field.
    for y in 0..life.height {
        for x in 0..life.width {
            if rng.chance(density) {
                life.set(x as isize, y as isize, true);
            }
        }
    }
    // A Gosper glider gun near the top-left for perpetual motion.
    if let Some(gun) = patterns::by_name("gosper-glider-gun") {
        if life.width > gun.width + 4 && life.height > gun.height + 4 {
            stamp(life, &gun, 3, 3);
        }
    }
    // Sprinkle a few spaceships heading across the field.
    let ships = ["glider", "lwss", "mwss", "hwss"];
    let n = (life.width * life.height / 4000).clamp(2, 12);
    for _ in 0..n {
        let name = ships[rng.below(ships.len())];
        if let Some(p) = patterns::by_name(name) {
            let ox = rng.below(life.width.saturating_sub(p.width + 1).max(1)) as isize;
            let oy = rng.below(life.height.saturating_sub(p.height + 1).max(1)) as isize;
            stamp(life, &p, ox, oy);
        }
    }
}

fn methuselah(life: &mut Life, rng: &mut Rng) {
    let names = ["r-pentomino", "acorn", "diehard"];
    let name = names[rng.below(names.len())];
    if let Some(p) = patterns::by_name(name) {
        let ox = (life.width / 2).saturating_sub(p.width / 2) as isize;
        let oy = (life.height / 2).saturating_sub(p.height / 2) as isize;
        stamp(life, &p, ox, oy);
    }
}
