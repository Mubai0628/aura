//! Aura's pixel pictures, kept as the design's own SVG paths: each path is a run of whole cells
//! ("M13 1h6v1h-6z" is six cells from column 13 on row 1), so a picture drawn at a whole number of
//! pixels per cell stays sharp.
use crate::theme::{self, Rgb};

/// A picture on a grid of `grid` cells. A layer without a colour takes the colour it is drawn in.
#[derive(Debug)]
pub struct Art {
    pub name: &'static str,
    pub grid: (i32, i32),
    pub layers: &'static [Layer],
}

#[derive(Debug)]
pub struct Layer {
    pub colour: Option<Rgb>,
    pub path: &'static str,
}

/// A rectangle of cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cells {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Layer {
    pub fn cells(&self) -> Vec<Cells> {
        parse(self.path)
    }
}

/// The rectangles of a path made only of closed runs: `M x y`, then `h`, `v`, `H` or `V`, then `z`.
fn parse(path: &str) -> Vec<Cells> {
    let mut tokens = Vec::new();
    let mut number = String::new();
    for character in path.chars() {
        if character.is_ascii_digit() || (character == '-' && number.is_empty()) {
            number.push(character);
            continue;
        }
        if !number.is_empty() {
            tokens.push(Token::Number(number.parse().unwrap_or(0)));
            number.clear();
        }
        if character.is_ascii_alphabetic() {
            tokens.push(Token::Command(character));
        }
    }
    if !number.is_empty() {
        tokens.push(Token::Number(number.parse().unwrap_or(0)));
    }
    let mut cells = Vec::new();
    let mut points: Vec<(i32, i32)> = Vec::new();
    let mut at = (0, 0);
    let mut items = tokens.into_iter().peekable();
    while let Some(token) = items.next() {
        let Token::Command(command) = token else { continue };
        let mut next = || match items.peek() {
            Some(Token::Number(value)) => {
                let value = *value;
                items.next();
                value
            }
            _ => 0,
        };
        match command {
            'M' => {
                let x = next();
                let y = next();
                at = (x, y);
                points = vec![at];
            }
            'h' => at.0 += next(),
            'v' => at.1 += next(),
            'H' => at.0 = next(),
            'V' => at.1 = next(),
            'z' | 'Z' => {
                let xs = points.iter().map(|point| point.0);
                let ys = points.iter().map(|point| point.1);
                let (left, right) = (xs.clone().min().unwrap_or(0), xs.max().unwrap_or(0));
                let (top, bottom) = (ys.clone().min().unwrap_or(0), ys.max().unwrap_or(0));
                if right > left && bottom > top {
                    cells.push(Cells {
                        x: left,
                        y: top,
                        width: right - left,
                        height: bottom - top,
                    });
                }
                points.clear();
                continue;
            }
            _ => {}
        }
        if command != 'M' {
            points.push(at);
        }
    }
    cells
}

enum Token {
    Command(char),
    Number(i32),
}

/// The Aura mark on a 32-cell grid.
pub static LOGO: Art = Art {
    name: "logo",
    grid: (32, 32),
    layers: &[Layer {
        colour: None,
        path: "M13 1h6v1h-6zM12 2h8v1h-8zM11 3h10v1h-10zM11 4h11v1h-11zM10 5h12v1h-12zM9 6h7v1h-7zM17 6h6v1h-6zM9 7h6v1h-6zM17 7h6v1h-6zM8 8h6v1h-6zM18 8h6v1h-6zM8 9h6v1h-6zM19 9h6v1h-6zM7 10h6v1h-6zM19 10h6v1h-6zM6 11h6v1h-6zM20 11h6v1h-6zM6 12h6v1h-6zM20 12h6v1h-6zM5 13h6v1h-6zM14 13h5v1h-5zM4 14h7v1h-7zM13 14h8v1h-8zM4 15h6v1h-6zM13 15h9v1h-9zM3 16h6v1h-6zM13 16h11v1h-11zM3 17h6v1h-6zM13 17h2v1h-2zM18 17h8v1h-8zM2 18h6v1h-6zM13 18h1v1h-1zM19 18h8v1h-8zM1 19h7v1h-7zM13 19h1v1h-1zM20 19h9v1h-9zM1 20h6v1h-6zM13 20h1v1h-1zM22 20h8v1h-8zM0 21h7v1h-7zM13 21h1v1h-1zM23 21h8v1h-8zM0 22h6v1h-6zM13 22h2v1h-2zM25 22h7v1h-7zM0 23h6v1h-6zM13 23h10v1h-10zM26 23h6v1h-6zM0 24h6v1h-6zM13 24h10v1h-10zM27 24h5v1h-5zM0 25h6v1h-6zM12 25h10v1h-10zM26 25h5v1h-5zM0 26h8v1h-8zM11 26h10v1h-10zM26 26h4v1h-4zM1 27h19v1h-19zM25 27h4v1h-4zM2 28h18v1h-18zM24 28h3v1h-3zM2 29h16v1h-16zM24 29h2v1h-2zM4 30h13v1h-13zM23 30h1v1h-1zM7 31h6v1h-6z",
    }],
};

pub static CLOSE: Art = Art {
    name: "close",
    grid: (16, 16),
    layers: &[Layer {
        colour: None,
        path: "M3 3h1v1h-1zM11 3h1v1h-1zM4 4h1v1h-1zM10 4h1v1h-1zM5 5h1v1h-1zM9 5h1v1h-1zM6 6h1v1h-1zM8 6h1v1h-1zM7 7h1v1h-1zM6 8h1v1h-1zM8 8h1v1h-1zM5 9h1v1h-1zM9 9h1v1h-1zM4 10h1v1h-1zM10 10h1v1h-1zM3 11h1v1h-1zM11 11h1v1h-1z",
    }],
};

/// A finished step.
pub static CHECK: Art = Art {
    name: "check",
    grid: (16, 16),
    layers: &[Layer {
        colour: None,
        path: "M12 4h1v1h-1zM11 5h1v1h-1zM10 6h1v1h-1zM3 7h1v1h-1zM9 7h1v1h-1zM4 8h1v1h-1zM8 8h1v1h-1zM5 9h1v1h-1zM7 9h1v1h-1zM6 10h1v1h-1z",
    }],
};

/// A step still to come.
pub static TODO: Art =
    Art { name: "todo", grid: (16, 16), layers: &[Layer { colour: None, path: "M7 7h2v2h-2z" }] };

pub static WARNING: Art = Art {
    name: "warning",
    grid: (16, 16),
    layers: &[Layer {
        colour: None,
        path: "M7 2h1v1h-1zM6 3h1v1h-1zM8 3h1v1h-1zM6 4h1v1h-1zM8 4h1v1h-1zM5 5h1v1h-1zM9 5h1v1h-1zM5 6h1v1h-1zM7 6h1v1h-1zM9 6h1v1h-1zM4 7h1v1h-1zM7 7h1v1h-1zM10 7h1v1h-1zM4 8h1v1h-1zM7 8h1v1h-1zM10 8h1v1h-1zM3 9h1v1h-1zM7 9h1v1h-1zM11 9h1v1h-1zM3 10h1v1h-1zM11 10h1v1h-1zM2 11h1v1h-1zM7 11h1v1h-1zM12 11h1v1h-1zM2 12h1v1h-1zM12 12h1v1h-1zM1 13h13v1h-13z",
    }],
};

/// The tick inside a checked box and a finished step's marker, white on the accent.
pub static TICK: Art = Art {
    name: "tick",
    grid: (12, 12),
    layers: &[Layer {
        colour: Some(theme::WHITE),
        path: "M8 3h1v1H8zM7 4h1v1H7zM2 5h1v1H2zM6 5h1v1H6zM3 6h1v1H3zM5 6h1v1H5zM4 7h1v1H4z",
    }],
};

/// The shark swimming over the sea, drawn two pixels to a cell.
pub static FISH: Art = Art {
    name: "fish",
    grid: (28, 11),
    layers: &[
        Layer {
            colour: Some(0x4b5059),
            path: "M12 0h2v1h-2zM11 1h3v1h-3zM10 2h4v1h-4zM0 3h2v1h-2zM9 3h5v1h-5zM1 4h2v1h-2zM5 4h16v1h-16zM2 5h17v1h-17zM20 5h1v1h-1zM2 6h20v1h-20zM1 7h2v1h-2zM25 7h1v1h-1zM0 8h2v1h-2zM12 8h2v1h-2zM11 9h2v1h-2zM10 10h1v1h-1z",
        },
        Layer {
            colour: Some(0x9097a0),
            path: "M21 5h2v1h-2zM22 6h3v1h-3zM4 7h21v1h-21zM7 8h5v1h-5zM14 8h10v1h-10z",
        },
        Layer { colour: Some(0xf1f2ef), path: "M19 5h1v1h-1z" },
    ],
};

/// The spinner's eight dots on a 16-cell grid, clockwise from the top, and how strong each is
/// when the first one leads; each tick moves the lead one dot on.
pub const SPINNER_DOTS: [(i32, i32); 8] =
    [(7, 2), (10, 4), (12, 7), (10, 10), (7, 12), (4, 10), (2, 7), (4, 4)];
pub const SPINNER_ALPHA: [f32; 8] = [1.0, 0.12, 0.18, 0.25, 0.34, 0.46, 0.6, 0.78];

/// How strong dot `dot` is at animation step `phase`.
pub fn spinner_alpha(dot: usize, phase: u32) -> f32 {
    SPINNER_ALPHA[(dot + 8 - (phase as usize % 8)) % 8]
}
