//! Curated library of iconic Game-of-Life patterns, embedded as RLE strings so
//! more can be dropped in verbatim from the LifeWiki. Grouped by category so the
//! "garden" scene can lay them out with labels.

use crate::rle::Pattern;

pub struct NamedPattern {
    pub name: &'static str,
    pub category: Category,
    pub rle: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    StillLife,
    Oscillator,
    Spaceship,
    Gun,
    Methuselah,
}

impl NamedPattern {
    pub fn pattern(&self) -> Pattern {
        Pattern::parse_rle(self.rle).unwrap_or_default()
    }
}

/// Fetch a pattern by (case-insensitive) name.
pub fn by_name(name: &str) -> Option<Pattern> {
    let n = name.to_ascii_lowercase();
    ALL.iter()
        .find(|p| p.name.eq_ignore_ascii_case(&n))
        .map(|p| p.pattern())
}

pub fn in_category(cat: Category) -> impl Iterator<Item = &'static NamedPattern> {
    ALL.iter().filter(move |p| p.category == cat)
}

use Category::*;

/// The library. RLE bodies are taken from the standard LifeWiki encodings.
pub static ALL: &[NamedPattern] = &[
    // ---- Still lifes ----
    NamedPattern { name: "block", category: StillLife, rle: "x=2,y=2\n2o$2o!" },
    NamedPattern { name: "beehive", category: StillLife, rle: "x=4,y=3\nb2ob$o2bo$b2o!" },
    NamedPattern { name: "loaf", category: StillLife, rle: "x=4,y=4\nb2ob$o2bo$bobo$2bo!" },
    NamedPattern { name: "boat", category: StillLife, rle: "x=3,y=3\n2ob$obo$bo!" },
    NamedPattern { name: "tub", category: StillLife, rle: "x=3,y=3\nbob$obo$bo!" },
    NamedPattern { name: "pond", category: StillLife, rle: "x=4,y=4\nb2ob$o2bo$o2bo$b2o!" },
    NamedPattern { name: "ship", category: StillLife, rle: "x=3,y=3\n2ob$obo$b2o!" },
    NamedPattern { name: "snake", category: StillLife, rle: "x=4,y=2\n2obo$ob2o!" },
    NamedPattern { name: "eater", category: StillLife, rle: "x=4,y=4\n2o2b$obo2b$2bob$2b2o!" },

    // ---- Oscillators ----
    NamedPattern { name: "blinker", category: Oscillator, rle: "x=3,y=1\n3o!" },
    NamedPattern { name: "toad", category: Oscillator, rle: "x=4,y=2\nb3o$3o!" },
    NamedPattern { name: "beacon", category: Oscillator, rle: "x=4,y=4\n2o2b$2o2b$2b2o$2b2o!" },
    NamedPattern { name: "pulsar", category: Oscillator, rle:
        "x=13,y=13\n2b3o3b3o2b2$o4bobo4bo$o4bobo4bo$o4bobo4bo$2b3o3b3o2b2$2b3o3b3o2b$o4bobo4bo$o4bobo4bo$o4bobo4bo2$2b3o3b3o!" },
    NamedPattern { name: "pentadecathlon", category: Oscillator, rle:
        "x=10,y=3\n2bo4bo2b$2ob4ob2o$2bo4bo!" },
    NamedPattern { name: "clock", category: Oscillator, rle: "x=4,y=4\n2bob$obob$bobo$bo!" },
    NamedPattern { name: "figure-eight", category: Oscillator, rle:
        "x=6,y=6\n3o3b$3o3b$3o3b$3b3o$3b3o$3b3o!" },
    NamedPattern { name: "tumbler", category: Oscillator, rle:
        "x=9,y=5\nbo5bob$obo3bobo$obo3bobo$2o5b2o2$!" },
    NamedPattern { name: "koks-galaxy", category: Oscillator, rle:
        "x=9,y=9\n6o2b$6obo$8o$b6o$9b$6o3b$8o$ob6o$2b6o!" },

    // ---- Spaceships ----
    NamedPattern { name: "glider", category: Spaceship, rle: "x=3,y=3\nbob$2bo$3o!" },
    NamedPattern { name: "lwss", category: Spaceship, rle:
        "x=5,y=4\nbo2bo$o4b$o3bo$4o!" },
    NamedPattern { name: "mwss", category: Spaceship, rle:
        "x=6,y=5\n3bo2b$bo3bo$o5b$o4bo$5o!" },
    NamedPattern { name: "hwss", category: Spaceship, rle:
        "x=7,y=5\n3b2o2b$bo4bo$o6b$o5bo$6o!" },

    // ---- Guns ----
    NamedPattern { name: "gosper-glider-gun", category: Gun, rle:
        "x=36,y=9\n24bo11b$22bobo11b$12b2o6b2o12b2o$11bo3bo4b2o12b2o$2o8bo5bo3b2o14b$2o8bo3bob2o4bobo11b$10bo5bo7bo11b$11bo3bo20b$12b2o!" },
    NamedPattern { name: "simkin-glider-gun", category: Gun, rle:
        "x=33,y=21\n2o5b2o$2o5b2o2$4b2o$4b2o5$22b2o2b2o$21bo4bo$21bo5bo b2o$21b3o3bo b2o$26bo4$20b2o$20bo$21b3o$23bo!" },

    // ---- Methuselahs ----
    NamedPattern { name: "r-pentomino", category: Methuselah, rle: "x=3,y=3\nb2o$2ob$bo!" },
    NamedPattern { name: "diehard", category: Methuselah, rle:
        "x=8,y=3\n6bob$2o6b$bo3b3o!" },
    NamedPattern { name: "acorn", category: Methuselah, rle:
        "x=7,y=3\nbo5b$3bo3b$2o2b3o!" },
];
