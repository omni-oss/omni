// ansi_paragraph.rs
// Incremental ANSI parser using `vte` to feed into `ratatui::Paragraph`.
// This version supports partial stream parsing: you can feed bytes as they arrive
// and get accumulated lines/spans.

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
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            performer: AnsiToSpans::new(),
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

#[derive(Clone, Default, Debug)]
struct AnsiToSpans {
    cur: AttributeState,
    buf: String,
    spans: Vec<Span<'static>>,
    lines: Vec<Line<'static>>,
}

impl AnsiToSpans {
    fn new() -> Self {
        Self {
            cur: AttributeState::default(),
            buf: String::new(),
            spans: Vec::new(),
            lines: Vec::new(),
        }
    }

    fn flush_buf(&mut self) {
        if !self.buf.is_empty() {
            let style = self.cur.to_style();
            let owned = Cow::Owned(self.buf.clone());
            self.spans.push(Span::styled(owned, style));
            self.buf.clear();
        }
    }

    fn newline(&mut self) {
        self.flush_buf();
        if !self.spans.is_empty() {
            let s = Line::from(self.spans.clone());
            self.lines.push(s);
            self.spans.clear();
        } else {
            self.lines.push(Line::from(vec![Span::raw("")]));
        }
    }

    fn snapshot(&mut self) -> Vec<Line<'static>> {
        // Return current lines + in-progress line
        let mut lines = self.lines.clone();
        if !self.buf.is_empty() || !self.spans.is_empty() {
            let mut temp_spans = self.spans.clone();
            if !self.buf.is_empty() {
                let style = self.cur.to_style();
                let owned = Cow::Owned(self.buf.clone());
                temp_spans.push(Span::styled(owned, style));
            }
            lines.push(Line::from(temp_spans));
        }
        lines
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_buf();
        if !self.spans.is_empty() {
            self.lines.push(Line::from(self.spans));
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
                1 => self.cur.bold = true,
                3 => self.cur.italic = true,
                4 => self.cur.underline = true,
                7 => self.cur.reversed = true,
                22 => self.cur.bold = false,
                23 => self.cur.italic = false,
                24 => self.cur.underline = false,
                27 => self.cur.reversed = false,
                30..=37 => {
                    self.cur.fg = Some(self.basic_color((p - 30) as u8, false));
                }
                40..=47 => {
                    self.cur.bg = Some(self.basic_color((p - 40) as u8, false));
                }
                90..=97 => {
                    self.cur.fg = Some(self.basic_color((p - 90) as u8, true));
                }
                100..=107 => {
                    self.cur.bg = Some(self.basic_color((p - 100) as u8, true));
                }
                38 | 48 => {
                    let is_fg = p == 38;
                    if i + 1 < params.len() {
                        let mode = params[i + 1];
                        if mode == 5 && i + 2 < params.len() {
                            let idx = params[i + 2] as u8;
                            let c = Color::Indexed(idx);
                            if is_fg {
                                self.cur.fg = Some(c);
                            } else {
                                self.cur.bg = Some(c);
                            }
                            i += 2;
                        } else if mode == 2 && i + 4 < params.len() {
                            let r = params[i + 2] as u8;
                            let g = params[i + 3] as u8;
                            let b = params[i + 4] as u8;
                            let c = Color::Rgb(r, g, b);
                            if is_fg {
                                self.cur.fg = Some(c);
                            } else {
                                self.cur.bg = Some(c);
                            }
                            i += 4;
                        }
                    }
                }
                39 => {
                    self.cur.fg = None;
                }
                49 => {
                    self.cur.bg = None;
                }
                _ => {}
            }
            i += 1;
        }

        trace::debug!("parsed state: {:?}", self.cur);
    }

    fn reset_all(&mut self) {
        self.cur.reset();
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
}

impl Perform for AnsiToSpans {
    fn print(&mut self, c: char) {
        self.buf.push(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            _ => {}
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

        if action == 'm' {
            trace::debug!("parsed params: {:?}", params);
            self.flush_buf();
            for p in params.iter() {
                self.handle_sgr(p);
            }
        }
    }
    fn esc_dispatch(&mut self, _i: &[u8], _ignore: bool, _b: u8) {}
    fn hook(&mut self, _: &Params, _: &[u8], _: bool, _: char) {}
    fn put(&mut self, _: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {}
}
