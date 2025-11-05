/// Parse a single key-value pair
pub fn parse_key_value<T, U>(
    s: &str,
) -> Result<(T, U), Box<dyn std::error::Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: std::error::Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` in `{s}`"))?;
    let key = &s[..pos];
    let mut value = s[pos + 1..].trim();

    // Remove optional quotes
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value = &value[1..value.len() - 1];
    }

    Ok((key.parse()?, value.parse()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_value() {
        assert_eq!(
            parse_key_value::<String, i32>("key=123").unwrap(),
            ("key".to_string(), "123".parse().unwrap())
        );
        assert_eq!(
            parse_key_value::<String, i32>("key=123").unwrap(),
            ("key".to_string(), "123".parse().unwrap())
        );
    }
}
