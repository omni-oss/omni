#![allow(unused)]

use ratatui::{
    prelude::*,
    widgets::{Block, Tabs, Widget},
};
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub struct ScrollableTabs<'a> {
    titles: Vec<String>,
    pub selected: usize,
    scroll: usize,

    // Store our own config
    block: Option<Block<'a>>,
    style: Style,
    highlight_style: Style,
    divider: &'a str,
    padding: (Line<'a>, Line<'a>),
}

impl<'a> ScrollableTabs<'a> {
    pub fn new<T: Into<String>>(titles: Vec<T>) -> Self {
        Self {
            titles: titles.into_iter().map(|t| t.into()).collect(),
            selected: 0,
            scroll: 0,
            block: None,
            style: Style::default(),
            highlight_style: Style::default(),
            divider: "|",
            padding: (Line::from(" "), Line::from(" ")),
        }
    }

    pub fn next(&mut self) {
        if self.selected + 1 < self.titles.len() {
            self.selected += 1;
        }
    }

    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select(mut self, index: usize) -> Self {
        self.selected = index.min(self.titles.len().saturating_sub(1));
        self
    }

    fn tab_width(&self, title: &str) -> u16 {
        UnicodeWidthStr::width(title) as u16
            + self.padding.0.width() as u16
            + self.padding.1.width() as u16
    }

    // === Builder methods (mirror Tabs API) ===
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    pub fn divider(mut self, divider: &'a str) -> Self {
        self.divider = divider;
        self
    }

    pub fn padding(
        mut self,
        left: impl Into<Line<'a>>,
        right: impl Into<Line<'a>>,
    ) -> Self {
        self.padding = (left.into(), right.into());
        self
    }
}

impl<'a> Widget for ScrollableTabs<'a> {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        let max_width = area.width;

        // Ensure selected tab fits
        if !self.titles.is_empty() {
            while self.selected >= self.scroll {
                let mut test_width = 0;
                for (i, t) in
                    self.titles[self.scroll..=self.selected].iter().enumerate()
                {
                    test_width += self.tab_width(t);
                    if i > 0 {
                        test_width +=
                            UnicodeWidthStr::width(self.divider) as u16;
                    }
                }
                if test_width > max_width {
                    self.scroll += 1;
                } else {
                    break;
                }
            }
        }

        // Collect visible slice
        let mut visible = Vec::new();
        let mut width_used = 0;
        for (i, t) in self.titles.iter().enumerate().skip(self.scroll) {
            let w = self.tab_width(t);
            let sep = if visible.is_empty() {
                0
            } else {
                UnicodeWidthStr::width(self.divider) as u16
            };
            if width_used + w + sep > max_width {
                break;
            }
            width_used += w + sep;
            visible.push(t.as_str());
        }

        // Build Tabs with stored config
        let mut tabs = Tabs::new(visible)
            .select(self.selected.saturating_sub(self.scroll))
            .style(self.style)
            .highlight_style(self.highlight_style)
            .divider(self.divider)
            .padding(self.padding.0.clone(), self.padding.1.clone());

        if let Some(block) = self.block {
            tabs = tabs.block(block);
        }

        tabs.render(area, buf);
    }
}
