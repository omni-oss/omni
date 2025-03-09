pub(crate) fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                match next {
                    'n' => {
                        result.push('\n');
                        chars.next();
                    }
                    'r' => {
                        result.push('\r');
                        chars.next();
                    }
                    't' => {
                        result.push('\t');
                        chars.next();
                    }
                    '"' => {
                        result.push('"');
                        chars.next();
                    }
                    '\'' => {
                        result.push('\'');
                        chars.next();
                    }
                    'b' => {
                        result.push('\x08');
                        chars.next();
                    }
                    'f' => {
                        result.push('\x0C');
                        chars.next();
                    }
                    'v' => {
                        result.push('\x0B');
                        chars.next();
                    }
                    '\\' => {
                        result.push('\\');
                        chars.next();
                    }
                    _ => result.push('\\'),
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(c);
        }
    }

    result
}
