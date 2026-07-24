//! Conway's Game of Life engine.
//!
//! Correct B3/S23 rules, double-buffered so a whole generation is computed from
//! the previous state (no in-place corruption). Tracks per-cell *age* (how many
//! generations a cell has been alive) and a *trail* countdown for recently-dead
//! cells, which the renderer uses to color the colony as a living thing.
//!
//! Edge model: toroidal wrap-around by default (`wrap = true`) — the world is a
//! seamless torus, which feels continuous and screensaver-like. With `wrap =
//! false` the boundary is fixed dead space, which is what showcase patterns like
//! guns and spaceships expect (gliders sail off and vanish instead of wrapping
//! back to collide with their own colony). See the module docs in `main.rs` for
//! the tradeoff discussion.

/// Max age we bother tracking (saturates). Keeps `age` in a small integer and
/// gives the palette a stable range to map onto.
pub const MAX_AGE: u16 = 255;
/// How many generations a dead cell keeps a fading trail.
pub const TRAIL_LEN: u8 = 6;

pub struct Life {
    pub width: usize,
    pub height: usize,
    pub wrap: bool,
    /// 1 = alive, 0 = dead, for the current generation.
    cur: Vec<u8>,
    /// Scratch buffer for the next generation.
    next: Vec<u8>,
    /// Generations a cell has been continuously alive (0 if dead), saturating.
    pub age: Vec<u16>,
    /// Countdown for a recently-dead cell's fading trail (0 = none).
    pub trail: Vec<u8>,
    pub generation: u64,
}

impl Life {
    pub fn new(width: usize, height: usize, wrap: bool) -> Self {
        let n = width.max(1) * height.max(1);
        Life {
            width: width.max(1),
            height: height.max(1),
            wrap,
            cur: vec![0; n],
            next: vec![0; n],
            age: vec![0; n],
            trail: vec![0; n],
            generation: 0,
        }
    }

    #[inline]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    #[inline]
    pub fn is_alive(&self, x: usize, y: usize) -> bool {
        self.cur[self.idx(x, y)] != 0
    }

    /// Set a cell alive/dead, resetting its age/trail. Coordinates out of range
    /// are ignored (callers stamp patterns that may overhang).
    pub fn set(&mut self, x: isize, y: isize, alive: bool) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let i = self.idx(x as usize, y as usize);
        self.cur[i] = alive as u8;
        self.age[i] = if alive { 1 } else { 0 };
        self.trail[i] = 0;
    }

    pub fn clear(&mut self) {
        self.cur.iter_mut().for_each(|c| *c = 0);
        self.next.iter_mut().for_each(|c| *c = 0);
        self.age.iter_mut().for_each(|c| *c = 0);
        self.trail.iter_mut().for_each(|c| *c = 0);
        self.generation = 0;
    }

    /// Count of live cells in the current generation.
    pub fn population(&self) -> usize {
        self.cur.iter().filter(|&&c| c != 0).count()
    }

    /// Advance one generation. Returns the number of cells that changed state
    /// (births + deaths), handy for the `--diag` harness.
    pub fn step(&mut self) -> usize {
        let w = self.width;
        let h = self.height;
        let mut changed = 0usize;

        for y in 0..h {
            for x in 0..w {
                let n = self.live_neighbors(x, y);
                let i = y * w + x;
                let alive = self.cur[i] != 0;
                let born = !alive && n == 3;
                let survive = alive && (n == 2 || n == 3);
                let next_alive = born || survive;
                self.next[i] = next_alive as u8;

                if next_alive {
                    self.age[i] = if alive {
                        self.age[i].saturating_add(1).min(MAX_AGE)
                    } else {
                        1
                    };
                    self.trail[i] = 0;
                    if born {
                        changed += 1;
                    }
                } else {
                    if alive {
                        self.trail[i] = TRAIL_LEN;
                        changed += 1;
                    } else if self.trail[i] > 0 {
                        self.trail[i] -= 1;
                    }
                    self.age[i] = 0;
                }
            }
        }
        std::mem::swap(&mut self.cur, &mut self.next);
        self.generation += 1;
        changed
    }

    #[inline]
    fn live_neighbors(&self, x: usize, y: usize) -> u8 {
        let w = self.width;
        let h = self.height;
        let mut count = 0u8;
        if self.wrap {
            // Precompute wrapped rows/cols.
            let xm = if x == 0 { w - 1 } else { x - 1 };
            let xp = if x + 1 == w { 0 } else { x + 1 };
            let ym = if y == 0 { h - 1 } else { y - 1 };
            let yp = if y + 1 == h { 0 } else { y + 1 };
            let cols = [xm, x, xp];
            let rows = [ym, yp];
            // The two full neighbor rows above and below.
            for &ry in &rows {
                let base = ry * w;
                count += self.cur[base + xm];
                count += self.cur[base + x];
                count += self.cur[base + xp];
            }
            let _ = cols;
            // Same row, left and right.
            let base = y * w;
            count += self.cur[base + xm];
            count += self.cur[base + xp];
        } else {
            let x0 = x as isize;
            let y0 = y as isize;
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x0 + dx;
                    let ny = y0 + dy;
                    if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                        continue;
                    }
                    count += self.cur[ny as usize * w + nx as usize];
                }
            }
        }
        count
    }

    /// Resize the world, preserving the top-left overlap (no stretching — we
    /// clamp/copy in place, as the resize lesson from the sibling project warns).
    pub fn resize(&mut self, width: usize, height: usize) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        let n = width * height;
        let mut cur = vec![0u8; n];
        let mut age = vec![0u16; n];
        let mut trail = vec![0u8; n];
        let cw = self.width.min(width);
        let ch = self.height.min(height);
        for y in 0..ch {
            for x in 0..cw {
                let src = y * self.width + x;
                let dst = y * width + x;
                cur[dst] = self.cur[src];
                age[dst] = self.age[src];
                trail[dst] = self.trail[src];
            }
        }
        self.width = width;
        self.height = height;
        self.cur = cur;
        self.next = vec![0u8; n];
        self.age = age;
        self.trail = trail;
    }
}
