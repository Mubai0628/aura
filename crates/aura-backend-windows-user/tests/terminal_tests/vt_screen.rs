//! A small ASCII screen for the large-output fixture. Repainting existing cells is not new output.
pub(super) fn render(bytes: &[u8], columns: usize, rows: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars().peekable();
    let mut screen = Screen {
        cells: vec![vec![' '; columns]; rows],
        history: String::new(),
        row: 0,
        column: 0,
        wrap: false,
        saved: (0, 0),
    };
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.next() {
                Some('[') => {
                    let mut parameters = String::new();
                    for ch in chars.by_ref() {
                        if ('@'..='~').contains(&ch) {
                            screen.csi(ch, &parameters);
                            break;
                        }
                        parameters.push(ch);
                    }
                }
                Some(']') => {
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' || (ch == '\x1b' && chars.next() == Some('\\')) {
                            break;
                        }
                    }
                }
                Some('7') => screen.saved = (screen.row, screen.column),
                Some('8') => {
                    (screen.row, screen.column) = screen.saved;
                    screen.wrap = false;
                }
                Some('M') => screen.row = screen.row.saturating_sub(1),
                Some(_) | None => {}
            },
            '\r' => {
                screen.column = 0;
                screen.wrap = false;
            }
            '\n' => {
                screen.newline();
                screen.wrap = false;
            }
            '\x08' => {
                screen.column = screen.column.saturating_sub(1);
                screen.wrap = false;
            }
            '\t' => screen.column = ((screen.column / 8 + 1) * 8).min(columns - 1),
            ch if ch >= ' ' => {
                if screen.wrap {
                    screen.column = 0;
                    screen.newline();
                    screen.wrap = false;
                }
                screen.cells[screen.row][screen.column] = ch;
                if screen.column + 1 == columns {
                    screen.wrap = true;
                } else {
                    screen.column += 1;
                }
            }
            _ => {}
        }
    }
    for row in screen.cells {
        screen.history.push_str(&row.iter().collect::<String>());
        screen.history.push('\n');
    }
    screen.history
}
struct Screen {
    cells: Vec<Vec<char>>,
    history: String,
    row: usize,
    column: usize,
    wrap: bool,
    saved: (usize, usize),
}
impl Screen {
    fn newline(&mut self) {
        if self.row + 1 == self.cells.len() {
            self.scroll(1);
        } else {
            self.row += 1;
        }
    }
    fn scroll(&mut self, count: usize) {
        for _ in 0..count.min(self.cells.len()) {
            self.history.push_str(&self.cells[0].iter().collect::<String>());
            self.history.push('\n');
            self.cells.rotate_left(1);
            self.cells.last_mut().expect("nonempty screen").fill(' ');
        }
    }
    fn csi(&mut self, code: char, values: &str) {
        if values.starts_with('?') {
            return;
        }
        let parameters =
            values.split(';').map(|value| value.parse::<usize>().unwrap_or(0)).collect::<Vec<_>>();
        let first = parameters.first().copied().unwrap_or(0);
        let count = first.max(1);
        let height = self.cells.len();
        let width = self.cells[0].len();
        match code {
            'H' | 'f' => {
                self.row = count.saturating_sub(1).min(height - 1);
                self.column =
                    parameters.get(1).copied().unwrap_or(1).max(1).saturating_sub(1).min(width - 1);
            }
            'A' => self.row = self.row.saturating_sub(count),
            'B' => self.row = (self.row + count).min(height - 1),
            'C' => self.column = (self.column + count).min(width - 1),
            'D' => self.column = self.column.saturating_sub(count),
            'E' => {
                self.row = (self.row + count).min(height - 1);
                self.column = 0;
            }
            'F' => {
                self.row = self.row.saturating_sub(count);
                self.column = 0;
            }
            'G' => self.column = count.saturating_sub(1).min(width - 1),
            'd' => self.row = count.saturating_sub(1).min(height - 1),
            'S' => self.scroll(count),
            'K' => match first {
                1 => self.cells[self.row][..=self.column].fill(' '),
                2 => self.cells[self.row].fill(' '),
                _ => self.cells[self.row][self.column..].fill(' '),
            },
            'J' => match first {
                2 | 3 => {
                    for row in &mut self.cells {
                        row.fill(' ');
                    }
                    if first == 3 {
                        self.history.clear();
                    }
                }
                1 => {
                    for row in &mut self.cells[..self.row] {
                        row.fill(' ');
                    }
                    self.cells[self.row][..=self.column].fill(' ');
                }
                _ => {
                    for row in &mut self.cells[self.row + 1..] {
                        row.fill(' ');
                    }
                    self.cells[self.row][self.column..].fill(' ');
                }
            },
            'X' => {
                let end = (self.column + count).min(width);
                self.cells[self.row][self.column..end].fill(' ');
            }
            's' => self.saved = (self.row, self.column),
            'u' => (self.row, self.column) = self.saved,
            'm' | 'n' | 't' | 'h' | 'l' => return,
            other => panic!("large-output screen fixture needs CSI {values}{other}"),
        }
        self.wrap = false;
    }
}
