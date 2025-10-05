use std::collections::HashSet;

/// Escapes a path component to be safe for cross-platform file systems.
/// Invalid characters are replaced with their Unicode escape sequence.
/// This keeps similar paths grouped together while ensuring uniqueness.
///
/// # Arguments
/// * `component` - The path component to escape
///
/// # Returns
/// A string safe for use on Windows, macOS, and Linux file systems
///
/// # Example
/// ```
/// use omni_path_utils::escape_path_component;
///
/// let safe = escape_path_component("file<name>.txt");
/// assert_eq!(safe, "file%3Cname%3E.txt");
/// ```
pub fn escape_path_component(component: &str) -> String {
    // Characters that are invalid on at least one major platform:
    // Windows: < > : " / \ | ? *
    // Also reserved: ASCII control chars (0-31), DEL (127)
    // Reserved names on Windows: CON, PRN, AUX, NUL, COM1-9, LPT1-9

    let invalid_chars: HashSet<char> =
        ['<', '>', ':', '"', '/', '\\', '|', '?', '*']
            .iter()
            .copied()
            .collect();

    let reserved_names: HashSet<&str> = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
        "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .iter()
    .copied()
    .collect();

    let mut result = String::with_capacity(component.len());

    for ch in component.chars() {
        if ch.is_control() || ch == '\u{007F}' || invalid_chars.contains(&ch) {
            // Use percent encoding for invalid characters
            result.push_str(&format!("%{:02X}", ch as u32));
        } else {
            result.push(ch);
        }
    }

    // Handle trailing dots and spaces (invalid on Windows)
    let trimmed = result.trim_end_matches(&['.', ' '][..]);
    if trimmed.is_empty() {
        // If the entire name was dots/spaces, encode them
        result = component
            .chars()
            .map(|ch| format!("%{:02X}", ch as u32))
            .collect();
    } else if trimmed.len() != result.len() {
        // Encode trailing dots and spaces
        let trailing = &result[trimmed.len()..].to_string();
        result = trimmed.to_string();
        for ch in trailing.chars() {
            result.push_str(&format!("%{:02X}", ch as u32));
        }
    }

    // Check if the name (without extension) is a reserved Windows name
    let name_without_ext = if let Some(pos) = result.rfind('.') {
        &result[..pos]
    } else {
        &result
    };

    if reserved_names.contains(name_without_ext.to_uppercase().as_str()) {
        // Prefix reserved names to make them safe
        result = format!("_{}", result);
    }

    // Ensure the result is not empty
    if result.is_empty() {
        result = "_".to_string();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_escaping() {
        assert_eq!(escape_path_component("normal.txt"), "normal.txt");
        assert_eq!(
            escape_path_component("file<name>.txt"),
            "file%3Cname%3E.txt"
        );
        assert_eq!(escape_path_component("file>name.txt"), "file%3Ename.txt");
        assert_eq!(escape_path_component("file:name.txt"), "file%3Aname.txt");
    }

    #[test]
    fn test_multiple_invalid_chars() {
        assert_eq!(
            escape_path_component("file<>:|?.txt"),
            "file%3C%3E%3A%7C%3F.txt"
        );
    }

    #[test]
    fn test_trailing_dots_and_spaces() {
        assert_eq!(escape_path_component("file.txt..."), "file.txt%2E%2E%2E");
        assert_eq!(escape_path_component("file.txt   "), "file.txt%20%20%20");
        assert_eq!(escape_path_component("..."), "%2E%2E%2E");
        assert_eq!(escape_path_component("   "), "%20%20%20");
    }

    #[test]
    fn test_reserved_names() {
        assert_eq!(escape_path_component("CON"), "_CON");
        assert_eq!(escape_path_component("con"), "_con");
        assert_eq!(escape_path_component("PRN.txt"), "_PRN.txt");
        assert_eq!(escape_path_component("COM1"), "_COM1");
        assert_eq!(escape_path_component("normal_con"), "normal_con");
    }

    #[test]
    fn test_control_characters() {
        assert_eq!(escape_path_component("file\x00name"), "file%00name");
        assert_eq!(escape_path_component("file\x1Fname"), "file%1Fname");
        assert_eq!(escape_path_component("file\x7Fname"), "file%7Fname");
    }

    #[test]
    fn test_empty_and_edge_cases() {
        assert_eq!(escape_path_component(""), "_");
        assert_eq!(escape_path_component("."), "%2E");
        assert_eq!(escape_path_component(".."), "%2E%2E");
    }

    #[test]
    fn test_unicode() {
        assert_eq!(escape_path_component("файл.txt"), "файл.txt");
        assert_eq!(escape_path_component("文件.txt"), "文件.txt");
        assert_eq!(escape_path_component("🎉.txt"), "🎉.txt");
    }

    #[test]
    fn test_similarity_preservation() {
        // Similar inputs should produce similar outputs
        let a = escape_path_component("file<1>.txt");
        let b = escape_path_component("file<2>.txt");
        let c = escape_path_component("file<3>.txt");

        // All should have the same prefix and similar structure
        assert!(a.starts_with("file%3C"));
        assert!(b.starts_with("file%3C"));
        assert!(c.starts_with("file%3C"));

        // But should be unique
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }
}
