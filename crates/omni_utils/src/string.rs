use normalize_line_endings as base;

pub fn normalize_line_endings(text: &str) -> impl Iterator<Item = char> {
    base::normalized(text.chars())
}
