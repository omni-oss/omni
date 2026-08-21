/// Overwrite `left` with `right` only when `right` is `Some`; otherwise leave
/// `left` untouched.
///
/// This is the "inherit-on-omit" merge strategy: an overriding layer that omits
/// the field (or sets it to `null`, which deserializes to `None`) inherits the
/// base value instead of clearing it. A concrete `Some` value replaces the base.
#[inline(always)]
pub fn replace_if_some<T>(left: &mut Option<T>, right: Option<T>) {
    if right.is_some() {
        *left = right;
    }
}
