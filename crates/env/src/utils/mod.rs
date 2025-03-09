pub(crate) fn is_valid_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
