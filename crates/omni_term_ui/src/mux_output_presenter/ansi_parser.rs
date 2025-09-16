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
        s
    }
}

#[derive(Default)]
pub struct AnsiParser {
    parser: Parser,
    performer: AnsiToSpans,
}

impl AnsiParser {
    #[inline(always)]
    #[allow(unused)]
    pub fn new(flags: Flags) -> Self {
        Self {
            parser: Parser::new(),
            performer: AnsiToSpans::new(flags),
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
struct AnsiToSpans {
    current_span_state: AttributeState,
    current_span_buf: String,
    current_line_spans: Vec<Span<'static>>,
    lines: Vec<Line<'static>>,
    cursor: Cursor,
    flags: Flags,
}

impl AnsiToSpans {
    fn new(flags: Flags) -> Self {
        Self {
            current_span_state: AttributeState::default(),
            current_span_buf: String::new(),
            current_line_spans: Vec::new(),
            lines: Vec::new(),
            cursor: Cursor::default(),
            flags,
        }
    }

    fn flush_span_buf(&mut self) {
        if !self.current_span_buf.is_empty() {
            let style = self.current_span_state.to_style();
            let owned = Cow::Owned(self.current_span_buf.clone());
            self.current_line_spans.push(Span::styled(owned, style));
            self.current_span_buf.clear();
        }
    }

    fn newline(&mut self) {
        self.flush_span_buf();
        if !self.current_line_spans.is_empty() {
            let s = Line::from(self.current_line_spans.clone());
            self.lines.push(s);
            self.current_line_spans.clear();
        } else {
            self.lines.push(Line::from(vec![Span::raw("")]));
        }
        self.cursor.nextline(&self.flags);
    }

    fn char(&mut self, c: char) {
        self.current_span_buf.push(c);
        self.cursor.x += 1;
    }

    fn snapshot(&self) -> Vec<Line<'static>> {
        // Return current lines + in-progress line
        let mut lines = self.lines.clone();
        if !self.current_span_buf.is_empty()
            || !self.current_line_spans.is_empty()
        {
            let mut temp_spans = self.current_line_spans.clone();
            if !self.current_span_buf.is_empty() {
                let style = self.current_span_state.to_style();
                let owned = Cow::Owned(self.current_span_buf.clone());
                temp_spans.push(Span::styled(owned, style));
            }
            lines.push(Line::from(temp_spans));
        }
        lines
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_span_buf();
        if !self.current_line_spans.is_empty() {
            self.lines.push(Line::from(self.current_line_spans));
        }
        self.lines
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
                3 => self.current_span_state.italic = true,
                4 => self.current_span_state.underline = true,
                7 => self.current_span_state.reversed = true,
                22 => self.current_span_state.bold = false,
                23 => self.current_span_state.italic = false,
                24 => self.current_span_state.underline = false,
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
                _ => {}
            }
            i += 1;
        }

        trace::trace!("parsed state: {:?}", self.current_span_state);
    }

    fn reset_all(&mut self) {
        self.current_span_state.reset();
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
            // 'H' => {}
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

impl Perform for AnsiToSpans {
    fn print(&mut self, c: char) {
        self.char(c);
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
                trace::trace!("parsed params: {:?}", params);
                self.flush_span_buf();
                for p in params.iter() {
                    self.handle_sgr(p);
                }
            }
            'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G' | 'd' | 'H' | 'f' | 's'
            | 'u' => {
                self.handle_cursor_movement(action, params);
            }
            c => {
                trace::warn!(
                    "unsupported action: {c}, if seen please contact the maintainers to add support"
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
