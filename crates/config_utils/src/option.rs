/// Overwrite `left` with `right` only if `left` is `None`.
pub fn replace_if_some<T>(left: &mut Option<T>, right: Option<T>) {
    if right.is_some() {
        *left = right;
    }
}
