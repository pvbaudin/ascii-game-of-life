//! Minimal parser for the standard Game-of-Life RLE (run-length encoded)
//! pattern format, so patterns can be embedded verbatim from the wiki.
//!
//! Format recap:
//!   - lines beginning with `#` are comments (`#N name`, `#C comment`, ...)
//!   - a header line `x = m, y = n, rule = B3/S23` (rule optional/ignored)
//!   - the body: run counts + tags. `b` = dead, `o` = alive, `$` = end of row,
//!     `!` = end of pattern. A number prefixes the tag as a repeat count.

/// A decoded pattern: a bounding box plus the coordinates of its live cells,
/// with the top-left of the bounding box at (0, 0).
#[derive(Clone, Debug, Default)]
pub struct Pattern {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<(usize, usize)>,
}

impl Pattern {
    /// Parse an RLE string. Tolerant: unknown tags and stray whitespace are
    /// skipped; a missing `!` still yields whatever was decoded.
    pub fn parse_rle(src: &str) -> Result<Pattern, String> {
        let mut header_w = 0usize;
        let mut header_h = 0usize;
        let mut body = String::new();

        for line in src.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('x') || line.starts_with('X') {
                // Parse `x = .., y = ..` (rule ignored).
                for part in line.split(',') {
                    let mut kv = part.splitn(2, '=');
                    let key = kv.next().unwrap_or("").trim().to_ascii_lowercase();
                    let val = kv.next().unwrap_or("").trim();
                    match key.as_str() {
                        "x" => header_w = val.parse().unwrap_or(0),
                        "y" => header_h = val.parse().unwrap_or(0),
                        _ => {}
                    }
                }
                continue;
            }
            body.push_str(line);
        }

        let mut cells = Vec::new();
        let mut count: usize = 0;
        let mut x = 0usize;
        let mut y = 0usize;
        let mut max_x = 0usize;

        for ch in body.chars() {
            match ch {
                '0'..='9' => {
                    count = count * 10 + (ch as usize - '0' as usize);
                }
                'b' | 'B' => {
                    x += count.max(1);
                    count = 0;
                }
                'o' | 'O' => {
                    let run = count.max(1);
                    for _ in 0..run {
                        cells.push((x, y));
                        x += 1;
                    }
                    count = 0;
                }
                '$' => {
                    let run = count.max(1);
                    max_x = max_x.max(x);
                    y += run;
                    x = 0;
                    count = 0;
                }
                '!' => break,
                _ => {} // ignore whitespace / unknown
            }
        }
        max_x = max_x.max(x);

        let width = header_w.max(max_x);
        let height = header_h.max(y + 1);
        if cells.is_empty() {
            return Err("no live cells decoded".into());
        }
        Ok(Pattern {
            width,
            height,
            cells,
        })
    }

    /// Build a pattern directly from a rectangular ASCII picture, where any of
    /// `o O * # @` is a live cell and everything else is dead. Handy for the
    /// built-in font and quick sketches.
    pub fn from_ascii(art: &str) -> Pattern {
        let mut cells = Vec::new();
        let mut width = 0usize;
        let mut height = 0usize;
        for (y, line) in art.lines().enumerate() {
            height = y + 1;
            for (x, ch) in line.chars().enumerate() {
                width = width.max(x + 1);
                if matches!(ch, 'o' | 'O' | '*' | '#' | '@') {
                    cells.push((x, y));
                }
            }
        }
        Pattern {
            width,
            height,
            cells,
        }
    }
}
