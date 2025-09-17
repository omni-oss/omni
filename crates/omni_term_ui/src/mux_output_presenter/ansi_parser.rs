// ansi_paragraph.rs
// Incremental ANSI parser using `vte` to feed into `ratatui::Paragraph`.
// This version supports partial stream parsing: you can feed bytes as they arrive
// and get accumulated lines/spans.

use enumflags2::{BitFlags, bitflags};
use ratatui::style::{Color, Modifier, Style, Stylize as _};
use ratatui::text::{Line, Span};
use std::borrow::Cow;
use vte::{Params, Parser, Perform};

#[derive(Clone, Debug, Default)]
struct AttributeState {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: bool,
    italic: bool,
    underline: bool,
    reversed: bool,
    dim: bool,
    hidden: bool,
    crossed_out: bool,
    blink: bool,
}

impl AttributeState {
    fn reset(&mut self) {
        *self = AttributeState::default();
    }

    fn to_style(&self) -> Style {
        let mut s = Style::default();
        if let Some(fg) = self.fg {
            s = s.fg(fg);
        }
        if let Some(bg) = self.bg {
            s = s.bg(bg);
        }
        let mut mods = Modifier::empty();
        if self.bold {
            mods.insert(Modifier::BOLD);
        }
        if self.italic {
            mods.insert(Modifier::ITALIC);
        }
        if self.underline {
            mods.insert(Modifier::UNDERLINED);
        }
        if mods != Modifier::empty() {
            s = s.add_modifier(mods);
        }
        if self.reversed {
            s = s.reversed();
        }
        if self.dim {
            s = s.dim();
        }
        if self.hidden {
            s = s.hidden();
        }
        if self.crossed_out {
            s = s.crossed_out();
        }
        if self.blink {
            s = s.slow_blink();
        }
        s
    }
}

#[derive(Default)]
pub struct AnsiParser {
    parser: Parser,
    performer: AnsiToGrid,
}

impl AnsiParser {
    #[inline(always)]
    #[allow(unused)]
    pub fn new(flags: Flags) -> Self {
        Self {
            parser: Parser::new(),
            performer: AnsiToGrid::new(flags),
        }
    }

    /// Feed a chunk of bytes. This can be partial and does not need to end on a full escape seq.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.performer, bytes);
    }

    /// Flush and get the current lines. This does not clear internal state,
    /// so you can keep feeding more and calling this again.
    pub fn snapshot(&mut self) -> Vec<Line<'static>> {
        self.performer.snapshot()
    }

    /// Finish parsing and drain all buffered content.
    #[inline(always)]
    #[allow(unused)]
    pub fn finish(self) -> Vec<Line<'static>> {
        self.performer.finish()
    }
}

#[bitflags]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flag {
    Onlcr,
}

pub type Flags = BitFlags<Flag>;

#[derive(Clone, Default, Debug, Copy)]
struct Cursor {
    x: usize,
    y: usize,
}

impl Cursor {
    fn nextline(&mut self, flags: &Flags) {
        if flags.contains(Flag::Onlcr) {
            self.x = 0;
        }
        self.y += 1;
    }

    fn nextchar(&mut self) {
        self.x += 1;
    }

    fn move_up(&mut self, movement: usize) {
        self.y = self.y.saturating_sub(movement);
    }

    fn carriage_return(&mut self) {
        self.x = 0;
    }

    fn move_down(&mut self, movement: usize) {
        self.y = self.y.saturating_add(movement);
    }

    fn move_right(&mut self, movement: usize) {
        self.x = self.x.saturating_add(movement);
    }

    fn move_left(&mut self, movement: usize) {
        self.x = self.x.saturating_sub(movement);
    }
}

#[derive(Clone, Default, Debug)]
struct Cell {
    content: char,
    style: Style,
}

#[derive(Clone, Default, Debug)]
struct AnsiToGrid {
    grid: Vec<Vec<Cell>>,
    current_span_state: AttributeState,
    cursor: Cursor,
    flags: Flags,
}

impl AnsiToGrid {
    fn new(flags: Flags) -> Self {
        let mut grid = vec![Vec::new()];
        let default_cell = Cell::default();
        grid[0].push(default_cell);
        Self {
            grid,
            current_span_state: AttributeState::default(),
            cursor: Cursor::default(),
            flags,
        }
    }

    fn ensure_cursor_in_bounds(&mut self) {
        if self.cursor.y >= self.grid.len() {
            self.grid.resize_with(self.cursor.y + 1, || Vec::new());
        }
        let line_len = self.grid[self.cursor.y].len();
        if self.cursor.x >= line_len {
            self.grid[self.cursor.y].resize_with(self.cursor.x + 1, || Cell {
                content: ' ',
                style: Style::default(),
            });
        }
    }

    fn print_char(&mut self, c: char) {
        self.ensure_cursor_in_bounds();
        let style = self.current_span_state.to_style();
        self.grid[self.cursor.y][self.cursor.x] = Cell { content: c, style };
        self.cursor.nextchar();
    }

    fn newline(&mut self) {
        self.cursor.nextline(&self.flags);
        self.ensure_cursor_in_bounds();
    }

    fn snapshot(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for row in &self.grid {
            let mut spans = Vec::new();
            let mut current_span_content = String::new();
            let mut current_span_style = None;

            for cell in row {
                let cell_style = Some(cell.style);
                if current_span_style.is_none() {
                    current_span_style = cell_style;
                }
                if current_span_style != cell_style {
                    if !current_span_content.is_empty() {
                        spans.push(Span::styled(
                            Cow::Owned(current_span_content.clone()),
                            current_span_style.unwrap_or_default(),
                        ));
                    }
                    current_span_content.clear();
                    current_span_style = cell_style;
                }
                current_span_content.push(cell.content);
            }
            if !current_span_content.is_empty() {
                spans.push(Span::styled(
                    Cow::Owned(current_span_content),
                    current_span_style.unwrap_or_default(),
                ));
            }
            lines.push(Line::from(spans));
        }
        lines
    }

    fn finish(self) -> Vec<Line<'static>> {
        self.snapshot()
    }

    fn handle_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset_all();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.reset_all(),
                1 => self.current_span_state.bold = true,
                2 => self.current_span_state.dim = true,
                3 => self.current_span_state.italic = true,
                4 => self.current_span_state.underline = true,
                5 => self.current_span_state.blink = true,
                7 => self.current_span_state.reversed = true,
                22 => self.current_span_state.bold = false,
                23 => self.current_span_state.italic = false,
                24 => self.current_span_state.underline = false,
                25 => self.current_span_state.blink = false,
                27 => self.current_span_state.reversed = false,
                30..=37 => {
                    self.current_span_state.fg =
                        Some(self.basic_color((p - 30) as u8, false));
                }
                40..=47 => {
                    self.current_span_state.bg =
                        Some(self.basic_color((p - 40) as u8, false));
                }
                90..=97 => {
                    self.current_span_state.fg =
                        Some(self.basic_color((p - 90) as u8, true));
                }
                100..=107 => {
                    self.current_span_state.bg =
                        Some(self.basic_color((p - 100) as u8, true));
                }
                38 | 48 => {
                    let is_fg = p == 38;
                    if i + 1 < params.len() {
                        let mode = params[i + 1];
                        if mode == 5 && i + 2 < params.len() {
                            let idx = params[i + 2] as u8;
                            let c = Color::Indexed(idx);
                            if is_fg {
                                self.current_span_state.fg = Some(c);
                            } else {
                                self.current_span_state.bg = Some(c);
                            }
                            i += 2;
                        } else if mode == 2 && i + 4 < params.len() {
                            let r = params[i + 2] as u8;
                            let g = params[i + 3] as u8;
                            let b = params[i + 4] as u8;
                            let c = Color::Rgb(r, g, b);
                            if is_fg {
                                self.current_span_state.fg = Some(c);
                            } else {
                                self.current_span_state.bg = Some(c);
                            }
                            i += 4;
                        }
                    }
                }
                39 => {
                    self.current_span_state.fg = None;
                }
                49 => {
                    self.current_span_state.bg = None;
                }
                u => {
                    trace::warn!("unknown escape sequence: {}", u);
                }
            }
            i += 1;
        }
    }

    fn reset_all(&mut self) {
        self.current_span_state.reset();
    }

    fn erase_in_line(&mut self, erase_opt: EraseFrom) {
        self.ensure_cursor_in_bounds();
        let line = &mut self.grid[self.cursor.y];
        match erase_opt {
            EraseFrom::CursorToEnd => {
                for i in self.cursor.x..line.len() {
                    line[i] = Cell {
                        content: ' ',
                        style: Style::default(),
                    };
                }
            }
            EraseFrom::StartToCursor => {
                for i in 0..self.cursor.x {
                    line[i] = Cell {
                        content: ' ',
                        style: Style::default(),
                    };
                }
            }
        }
    }

    fn erase_line(&mut self, params: &Params) {
        for param in params {
            for i in param {
                match i {
                    0 => {
                        self.erase_in_line(EraseFrom::CursorToEnd);
                    }
                    1 => {
                        self.erase_in_line(EraseFrom::StartToCursor);
                    }
                    2 => {
                        self.grid[self.cursor.y].clear();
                        self.cursor.x = 0;
                    }
                    _ => {
                        // trace::warn!("unsupported erase line param: {i}");
                    }
                }
            }
        }
    }

    fn handle_cursor_movement(&mut self, action: char, params: &Params) {
        fn params_sum(params: &Params) -> usize {
            let mut sum = 0;
            for param in params {
                sum += param.iter().sum::<u16>() as usize;
            }
            sum
        }

        match action {
            'A' => {
                let movement = params_sum(params);
                self.cursor.move_up(movement);
            }
            'B' => {
                let movement = params_sum(params);
                self.cursor.move_down(movement);
            }
            'C' => {
                let movement = params_sum(params);
                self.cursor.move_right(movement);
            }
            'D' => {
                let movement = params_sum(params);
                self.cursor.move_left(movement);
            }
            'E' => {
                let movement = params_sum(params);
                self.cursor.move_down(movement);
                self.cursor.carriage_return();
            }
            'F' => {
                let movement = params_sum(params);
                self.cursor.move_up(movement);
                self.cursor.carriage_return();
            }
            'G' => {
                let new_pos = params_sum(params);
                self.cursor.x = new_pos - 1; // translate to 0-based
            }
            // the following are unsupported for now
            // TODO: implement when needed
            // 'd' => {}
            // 'H' => {
            // }
            // 'f' => {}
            // 's' => {}
            // 'u' => {}
            c => {
                trace::warn!(
                    "unsupported movement action: {c}, if seen please contact the maintainers to add support"
                );
            }
        }
    }

    fn basic_color(&self, idx: u8, bright: bool) -> Color {
        match (idx, bright) {
            (0, false) => Color::Black,
            (1, false) => Color::Red,
            (2, false) => Color::Green,
            (3, false) => Color::Yellow,
            (4, false) => Color::Blue,
            (5, false) => Color::Magenta,
            (6, false) => Color::Cyan,
            (7, false) => Color::Gray,
            (0, true) => Color::DarkGray,
            (1, true) => Color::LightRed,
            (2, true) => Color::LightGreen,
            (3, true) => Color::LightYellow,
            (4, true) => Color::LightBlue,
            (5, true) => Color::LightMagenta,
            (6, true) => Color::LightCyan,
            (7, true) => Color::White,
            _ => Color::Reset,
        }
    }

    fn carriage_return(&mut self) {
        self.cursor.carriage_return();
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
enum EraseFrom {
    CursorToEnd,
    StartToCursor,
}

impl Perform for AnsiToGrid {
    fn print(&mut self, c: char) {
        self.print_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.carriage_return(),
            b => {
                trace::warn!(
                    "unsupported byte: {b:?}, if seen please contact the maintainers to add support"
                );
            }
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _i: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        match action {
            'm' => {
                for param in params {
                    self.handle_sgr(param);
                }
            }
            'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G' | 'd' | 'H' | 'f' | 's'
            | 'u' => {
                self.handle_cursor_movement(action, params);
            }
            'K' => {
                self.erase_line(params);
            }
            'h' => {
                // explicitly ignore h
            }
            'l' => {
                // explicitly ignore l
            }
            c => {
                trace::warn!(
                    "unsupported CSI action: {c} (params = {params:?}), if seen please contact the maintainers to add support"
                );
            }
        }
    }
    fn esc_dispatch(&mut self, _i: &[u8], _ignore: bool, _b: u8) {}
    fn hook(&mut self, _: &Params, _: &[u8], _: bool, _: char) {}
    fn put(&mut self, _: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {}
}
