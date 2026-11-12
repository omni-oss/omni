use omni_retry_config::RetryConfiguration;

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

/// Parse a retry spec: `COUNT[,strategy][,key=value...]`, for example
/// `3,exponential,base=500ms,max_delay=30s,jitter=full`.
///
/// The text is only split into a JSON object; the configuration's own
/// deserializer decides which keys and values are valid.
pub fn parse_retry_spec(s: &str) -> Result<RetryConfiguration, String> {
    let mut parts = s.split(',');
    let count = parts.next().unwrap_or_default();
    count.parse::<u8>().map_err(|_| {
        format!("first item must be the retry count (0-255), got `{count}`")
    })?;

    let mut obj = serde_json::Map::new();
    obj.insert("max_retries".into(), scalar(count));
    for part in parts {
        let (key, value) = part.split_once('=').unwrap_or(("strategy", part));
        if obj.insert(key.into(), scalar(value)).is_some() {
            return Err(format!("`{key}` given more than once"));
        }
    }

    let config: RetryConfiguration =
        serde_json::from_value(obj.into()).map_err(|e| e.to_string())?;
    config.check_layer().map_err(|e| e.to_string())?;
    Ok(config)
}

/// `3` becomes a number, `500ms` and `full` stay strings.
fn scalar(v: &str) -> serde_json::Value {
    serde_json::from_str(v).unwrap_or_else(|_| v.into())
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
    #[test]
    fn retry_spec_valid_forms() {
        let c = parse_retry_spec("3").unwrap();
        assert_eq!(c.into_policy().unwrap().max_retries, 3);

        let p = parse_retry_spec(
            "5,linear,base=500ms,step=2s,max_delay=30s,jitter=full",
        )
        .unwrap()
        .into_policy()
        .unwrap();
        assert_eq!(p.max_retries, 5);
        assert_eq!(p.base, std::time::Duration::from_millis(500));

        assert!(parse_retry_spec("255,factor=10").is_ok());
        assert!(parse_retry_spec("3,fixed").is_ok());
        assert!(parse_retry_spec("0").is_ok());
    }

    #[test]
    fn retry_spec_errors() {
        for bad in [
            "",
            "abc",
            "256",
            "3,expontential",
            "3,base=abc",
            "3,bogus=1",
            "3,exponential,step=1s",
            "3,fixed,factor=2",
            "3,factor=0.5",
            "3,base=1s,base=2s",
            "3,exponential,fixed",
        ] {
            assert!(parse_retry_spec(bad).is_err(), "`{bad}` should fail");
        }
    }
}
