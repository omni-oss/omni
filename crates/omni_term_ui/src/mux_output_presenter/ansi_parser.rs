// ansi_paragraph.rs
// Incremental ANSI parser using `vte` to feed into `ratatui::Paragraph`.
//
// Key performance changes vs. the original:
//
//  1. Completed lines are encoded into `Line<'static>` exactly once, at the
//     moment the cursor leaves them (newline / cursor-up past them).  They are
//     stored in `completed_lines` and never re-encoded.
//
//  2. Only the *current* (hot) row is stored as a `Vec<Cell>` and re-encoded
//     on each `snapshot()` call.  For a typical terminal this is at most one
//     line per frame, not the entire history.
//
//  3. `snapshot_range(offset, len)` lets the caller ask for only the lines
//     that will actually be visible, so ratatui never has to touch the rest.
//     `snapshot_line_count()` returns the total cheaply without any allocation.
//
//  4. Cursor-up movements that re-enter a completed line promote it back into
//     `active_row` so edits still work correctly, then re-bake it on the next
//     newline.  This is rare (progress-bar style output) and correct.
//
//  5. A `MAX_LINES` cap (ring-buffer eviction via VecDeque) prevents unbounded
//     memory growth for long-running processes.

use std::borrow::Cow;
use std::collections::VecDeque;

use enumflags2::{BitFlags, bitflags};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use vte::{Params, Parser, Perform};

// ---------------------------------------------------------------------------
// Tuneable constants
// ---------------------------------------------------------------------------

/// Maximum number of completed lines kept in memory.  Oldest lines are
/// dropped when this is exceeded.  Raise if you need more scroll-back.
const MAX_LINES: usize = 50_000;

// ---------------------------------------------------------------------------
// AttributeState
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
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

// ---------------------------------------------------------------------------
// Public API – AnsiParser
// ---------------------------------------------------------------------------

#[bitflags]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flag {
    Onlcr,
}

pub type Flags = BitFlags<Flag>;

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

    /// Feed a chunk of bytes (may be partial / mid-escape-sequence).
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.performer, bytes);
    }

    /// Total number of lines (completed + the current active row).
    /// O(1) — no allocation.
    #[inline]
    pub fn snapshot_line_count(&self) -> usize {
        self.performer.line_count()
    }

    /// Return only the lines in `[offset, offset + len)`.
    /// Clones at most `len` `Line` values (cheap Arc-style clone for completed
    /// lines; one fresh encode for the active row if it falls in range).
    ///
    /// Pass this instead of the full snapshot so ratatui only gets what it
    /// can actually render.
    pub fn snapshot_range(
        &self,
        offset: usize,
        len: usize,
    ) -> Vec<Line<'static>> {
        self.performer.snapshot_range(offset, len)
    }

    /// Legacy helper: returns *all* lines.  Prefer `snapshot_range` for the
    /// render path; this is O(total_lines).
    pub fn snapshot(&self) -> Vec<Line<'static>> {
        let total = self.performer.line_count();
        self.performer.snapshot_range(0, total)
    }

    /// Finish parsing and drain all buffered content.
    #[allow(unused)]
    pub fn finish(self) -> Vec<Line<'static>> {
        self.snapshot()
    }
}

// ---------------------------------------------------------------------------
// Internal grid
// ---------------------------------------------------------------------------

#[derive(Clone, Default, Debug, Copy)]
struct Cursor {
    /// Column (0-based).
    x: usize,
    /// Row *within the active region*.  0 = current active row.
    /// Positive values mean the cursor has moved *up* into previously
    /// completed lines that have been re-promoted to editable cells.
    ///
    /// We track absolute row as `completed_lines.len() - cursor_above + active_row_abs`.
    /// Simpler: we store an absolute row index where 0 is the very first line
    /// ever written.
    abs_y: usize,
}

#[derive(Clone, Default, Debug)]
struct Cell {
    content: char,
    style: Style,
}

/// Convert a `Vec<Cell>` row into a ratatui `Line<'static>`.
fn row_to_line(row: &[Cell]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_content = String::new();
    let mut current_style: Option<Style> = None;

    for cell in row {
        let cell_style = cell.style;
        match current_style {
            None => {
                current_style = Some(cell_style);
            }
            Some(s) if s != cell_style => {
                if !current_content.is_empty() {
                    spans.push(Span::styled(
                        Cow::Owned(std::mem::take(&mut current_content)),
                        s,
                    ));
                }
                current_style = Some(cell_style);
            }
            _ => {}
        }
        current_content.push(cell.content);
    }
    if !current_content.is_empty() {
        spans.push(Span::styled(
            Cow::Owned(current_content),
            current_style.unwrap_or_default(),
        ));
    }
    Line::from(spans)
}

// ---------------------------------------------------------------------------

#[derive(Default)]
struct AnsiToGrid {
    /// Permanently encoded lines.
    completed_lines: VecDeque<Line<'static>>,

    /// The row the cursor is currently writing into.
    active_row: Vec<Cell>,

    /// Absolute row index of `active_row`.  Starts at 0.
    active_abs_y: usize,

    /// The absolute row that `ESC[1;1H` (cursor-home) maps to.
    /// Advances as the terminal's "screen" scrolls so that programs
    /// which redraw in-place (htop, progress bars, cargo build, etc.)
    /// overwrite existing lines rather than appending new ones forever.
    viewport_top: usize,

    current_span_state: AttributeState,
    cursor: Cursor,
    flags: Flags,
}

impl AnsiToGrid {
    fn new(flags: Flags) -> Self {
        Self {
            flags,
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------
    // Geometry helpers
    // -----------------------------------------------------------------------

    fn line_count(&self) -> usize {
        // completed + active row (always at least 1)
        self.completed_lines.len() + 1
    }

    // -----------------------------------------------------------------------
    // Cursor / row management
    // -----------------------------------------------------------------------

    /// Make sure `completed_lines` reaches up to (but not including)
    /// `active_abs_y`, padding with empty lines if needed.
    fn sync_completed_len(&mut self) {
        while self.completed_lines.len() < self.active_abs_y {
            self.completed_lines.push_back(Line::default());
        }
        // Evict oldest lines if over cap, keeping all abs indices consistent.
        while self.completed_lines.len() > MAX_LINES {
            self.completed_lines.pop_front();
            self.active_abs_y = self.active_abs_y.saturating_sub(1);
            self.viewport_top = self.viewport_top.saturating_sub(1);
            self.cursor.abs_y = self.cursor.abs_y.saturating_sub(1);
        }
    }

    /// Bake the active row into `completed_lines` and start a new active row.
    fn bake_active_row(&mut self) {
        self.sync_completed_len();
        let baked = row_to_line(&self.active_row);
        if self.active_abs_y < self.completed_lines.len() {
            // Overwrite a previously-promoted row.
            self.completed_lines[self.active_abs_y] = baked;
        } else {
            self.completed_lines.push_back(baked);
        }
        self.active_abs_y += 1;
        self.active_row.clear();
        self.cursor.x = if self.flags.contains(Flag::Onlcr) {
            0
        } else {
            self.cursor.x
        };
        self.cursor.abs_y = self.active_abs_y;
    }

    fn ensure_active_row_wide_enough(&mut self) {
        if self.cursor.x >= self.active_row.len() {
            self.active_row.resize_with(self.cursor.x + 1, || Cell {
                content: ' ',
                style: Style::default(),
            });
        }
    }

    fn print_char(&mut self, c: char) {
        // If cursor is not on the active row, resolve it.
        if self.cursor.abs_y != self.active_abs_y {
            self.reposition_to_active();
        }
        self.ensure_active_row_wide_enough();
        let style = self.current_span_state.to_style();
        self.active_row[self.cursor.x] = Cell { content: c, style };
        self.cursor.x += 1;
    }

    fn newline(&mut self) {
        if self.cursor.abs_y != self.active_abs_y {
            self.reposition_to_active();
        }
        self.bake_active_row();
    }

    fn carriage_return(&mut self) {
        self.cursor.x = 0;
    }

    // -----------------------------------------------------------------------
    // Cursor movement
    // -----------------------------------------------------------------------

    fn cursor_move_up(&mut self, n: usize) {
        self.cursor.abs_y = self.cursor.abs_y.saturating_sub(n);
    }

    fn cursor_move_down(&mut self, n: usize) {
        self.cursor.abs_y = self.cursor.abs_y.saturating_add(n);
        // If we moved past the active row, extend.
        if self.cursor.abs_y > self.active_abs_y {
            // bake everything in between
            while self.active_abs_y < self.cursor.abs_y {
                self.bake_active_row();
            }
        }
    }

    /// Called when we're about to write but the cursor is not on `active_abs_y`.
    /// Promotes the target row (from completed_lines) to be the new active row,
    /// baking the previous active row first.
    fn reposition_to_active(&mut self) {
        let target = self.cursor.abs_y;

        // Bake current active row first.
        self.sync_completed_len();
        let baked = row_to_line(&self.active_row);
        if self.active_abs_y < self.completed_lines.len() {
            self.completed_lines[self.active_abs_y] = baked;
        } else {
            self.completed_lines.push_back(baked);
        }

        // Promote target row to active.
        if target < self.completed_lines.len() {
            // We need the raw cells – but we only have the baked Line.
            // Reconstruct a best-effort Vec<Cell> from the Line's spans.
            let line = std::mem::replace(
                &mut self.completed_lines[target],
                Line::default(),
            );
            self.active_row = line_to_cells(line);
        } else {
            self.active_row = Vec::new();
        }
        self.active_abs_y = target;
    }

    // -----------------------------------------------------------------------
    // Erase
    // -----------------------------------------------------------------------

    fn erase_in_line_cursor_to_end(&mut self) {
        if self.cursor.abs_y != self.active_abs_y {
            self.reposition_to_active();
        }
        self.ensure_active_row_wide_enough();
        for cell in &mut self.active_row[self.cursor.x..] {
            *cell = Cell {
                content: ' ',
                style: Style::default(),
            };
        }
    }

    fn erase_in_line_start_to_cursor(&mut self) {
        if self.cursor.abs_y != self.active_abs_y {
            self.reposition_to_active();
        }
        self.ensure_active_row_wide_enough();
        for cell in &mut self.active_row[..self.cursor.x] {
            *cell = Cell {
                content: ' ',
                style: Style::default(),
            };
        }
    }

    fn erase_line_all(&mut self) {
        if self.cursor.abs_y != self.active_abs_y {
            self.reposition_to_active();
        }
        self.active_row.clear();
        self.cursor.x = 0;
    }

    fn erase_line(&mut self, params: &Params) {
        for param in params {
            for i in param {
                match i {
                    0 => self.erase_in_line_cursor_to_end(),
                    1 => self.erase_in_line_start_to_cursor(),
                    2 => self.erase_line_all(),
                    _ => {
                        trace::warn!("unsupported erase line param: {i}");
                    }
                }
            }
        }
    }

    /// CSI J — Erase in Display.
    fn erase_in_display(&mut self, params: &Params) {
        let mode = params
            .iter()
            .flat_map(|p| p.iter())
            .next()
            .copied()
            .unwrap_or(0);
        match mode {
            // 0J: cursor to end of screen
            0 => {
                self.erase_in_line_cursor_to_end();
                // Blank all completed lines below the cursor.
                let start = self.active_abs_y + 1;
                for abs_y in start..self.completed_lines.len() {
                    self.completed_lines[abs_y] = Line::default();
                }
            }
            // 1J: start of screen to cursor
            1 => {
                // Blank all completed lines above the cursor (within viewport).
                for abs_y in self.viewport_top..self.active_abs_y {
                    if abs_y < self.completed_lines.len() {
                        self.completed_lines[abs_y] = Line::default();
                    }
                }
                self.erase_in_line_start_to_cursor();
            }
            // 2J: erase entire visible screen, keep scrollback
            2 => {
                for abs_y in self.viewport_top..=self.active_abs_y {
                    if abs_y < self.completed_lines.len() {
                        self.completed_lines[abs_y] = Line::default();
                    }
                }
                self.active_row.clear();
                self.cursor.x = 0;
            }
            // 3J: erase screen AND scrollback
            3 => {
                self.completed_lines.clear();
                self.active_row.clear();
                self.active_abs_y = 0;
                self.viewport_top = 0;
                self.cursor.abs_y = 0;
                self.cursor.x = 0;
            }
            _ => {
                trace::warn!("unsupported erase in display mode: {mode}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Snapshot
    // -----------------------------------------------------------------------

    fn snapshot_range(&self, offset: usize, len: usize) -> Vec<Line<'static>> {
        let total = self.line_count();
        if offset >= total || len == 0 {
            return Vec::new();
        }
        let end = (offset + len).min(total);
        let mut out = Vec::with_capacity(end - offset);

        let completed_len = self.completed_lines.len();

        for abs_y in offset..end {
            if abs_y < completed_len {
                out.push(self.completed_lines[abs_y].clone());
            } else {
                // This is the active row.
                out.push(row_to_line(&self.active_row));
            }
        }
        out
    }

    // -----------------------------------------------------------------------
    // SGR
    // -----------------------------------------------------------------------

    fn handle_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.current_span_state.reset();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.current_span_state.reset(),
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
                        Some(basic_color((p - 30) as u8, false));
                }
                40..=47 => {
                    self.current_span_state.bg =
                        Some(basic_color((p - 40) as u8, false));
                }
                90..=97 => {
                    self.current_span_state.fg =
                        Some(basic_color((p - 90) as u8, true));
                }
                100..=107 => {
                    self.current_span_state.bg =
                        Some(basic_color((p - 100) as u8, true));
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
                39 => self.current_span_state.fg = None,
                49 => self.current_span_state.bg = None,
                u => {
                    trace::warn!("unknown escape sequence: {}", u);
                }
            }
            i += 1;
        }
    }

    // -----------------------------------------------------------------------
    // Cursor movement dispatch
    // -----------------------------------------------------------------------

    fn handle_cursor_movement(&mut self, action: char, params: &Params) {
        fn params_sum(params: &Params) -> usize {
            params
                .iter()
                .flat_map(|p| p.iter())
                .map(|v| *v as usize)
                .sum()
        }
        fn params_sum_min1(params: &Params) -> usize {
            params_sum(params).max(1)
        }

        // Helper: extract the two semicolon-separated params for H/f.
        // vte gives `ESC[row;colH` as two sub-param groups [[row], [col]].
        fn two_params(params: &Params) -> (usize, usize) {
            let mut iter = params.iter();
            let row = iter
                .next()
                .and_then(|p| p.iter().next())
                .map(|v| *v as usize)
                .unwrap_or(0);
            let col = iter
                .next()
                .and_then(|p| p.iter().next())
                .map(|v| *v as usize)
                .unwrap_or(0);
            (row, col)
        }

        match action {
            'A' => self.cursor_move_up(params_sum_min1(params)),
            'B' => self.cursor_move_down(params_sum_min1(params)),
            'C' => {
                self.cursor.x =
                    self.cursor.x.saturating_add(params_sum_min1(params))
            }
            'D' => {
                self.cursor.x =
                    self.cursor.x.saturating_sub(params_sum_min1(params))
            }
            'E' => {
                self.cursor_move_down(params_sum_min1(params));
                self.cursor.x = 0;
            }
            'F' => {
                self.cursor_move_up(params_sum_min1(params));
                self.cursor.x = 0;
            }
            'G' => {
                let col = params_sum(params);
                self.cursor.x = col.saturating_sub(1);
            }
            // H / f — Cursor Position: ESC[row;colH  (1-based, default 1;1)
            'H' | 'f' => {
                let (row, col) = two_params(params);
                // 0 and 1 both mean "first" in 1-based terminal coords.
                let row = row.max(1);
                let col = col.max(1);

                // When homing to the top-left, anchor viewport_top here so
                // that subsequent in-place redraws overwrite rather than append.
                if row == 1 && col == 1 {
                    self.viewport_top = self.active_abs_y;
                }

                self.cursor.abs_y = self.viewport_top + (row - 1);
                self.cursor.x = col - 1;

                // If the target is below the current active row, extend.
                if self.cursor.abs_y > self.active_abs_y {
                    while self.active_abs_y < self.cursor.abs_y {
                        self.bake_active_row();
                    }
                }
            }
            c => {
                trace::warn!(
                    "unsupported movement action: {c}, if seen please contact the maintainers to add support"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn basic_color(idx: u8, bright: bool) -> Color {
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

/// Reconstruct `Vec<Cell>` from a `Line<'static>` so that cursor-up into a
/// completed line can re-edit it.  Style info is preserved; only called in
/// the (rare) cursor-up-into-completed path.
fn line_to_cells(line: Line<'static>) -> Vec<Cell> {
    let mut cells = Vec::new();
    for span in line.spans {
        let style = span.style;
        for c in span.content.chars() {
            cells.push(Cell { content: c, style });
        }
    }
    cells
}

// ---------------------------------------------------------------------------
// vte Perform
// ---------------------------------------------------------------------------

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
            'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G' | 'H' | 'f' | 'd' | 's'
            | 'u' => {
                self.handle_cursor_movement(action, params);
            }
            'J' => {
                self.erase_in_display(params);
            }
            'K' => {
                self.erase_line(params);
            }
            'h' | 'l' => { /* ignore */ }
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
