pub macro env {
    ($name:expr) => {{
        let rt_var = std::env::var($name);

        match rt_var {
            Ok(o) => Ok(o),
            Err(e) => {
                if matches!(e, std::env::VarError::NotPresent) {
                    let cmp_var = option_env!($name).map(|e| e.to_string());

                    if let Some(var) = cmp_var {
                        Ok(var)
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }},
    ($name:expr, $default:expr) => {{
        let e = env!($name);

        match e {
            Ok(o) => Ok(o),
            Err(e) => {
                if matches!(e, std::env::VarError::NotPresent) {
                    Ok($default.to_string())
                } else {
                    Err(e)
                }
            }
        }
    }}
}

pub(crate) fn get_service_and_user(
    default_service: Option<&str>,
    default_user: Option<&str>,
) -> Result<(String, String), std::env::VarError> {
    let service = env!(
        "OMNI_REMOTE_CACHE_CLIENT_SERVICE_NAME",
        default_service.unwrap_or("omni-remote-cache-client")
    )?;
    let user = env!(
        "OMNI_REMOTE_CACHE_CLIENT_USER_NAME",
        default_user.unwrap_or("default")
    )?;

    Ok((service, user))
}

#[cfg(test)]
mod tests {
    use super::env;

    // Each test owns a distinct variable name, so parallel tests do not race.
    #[test]
    fn env_reads_the_named_variable() {
        unsafe { std::env::set_var("OMNI_SETUP_TEST_NAMED_VAR", "set") };
        let value = env!("OMNI_SETUP_TEST_NAMED_VAR", "default").unwrap();
        unsafe { std::env::remove_var("OMNI_SETUP_TEST_NAMED_VAR") };
        assert_eq!(value, "set");
    }

    #[test]
    fn env_falls_back_to_the_default_when_unset() {
        let value = env!("OMNI_SETUP_TEST_UNSET_VAR", "default").unwrap();
        assert_eq!(value, "default");
        assert!(env!("OMNI_SETUP_TEST_UNSET_VAR").is_err());
    }
}
