pub macro env {
    ($name:expr) => {{
        let rt_var = std::env::var("e");

        match rt_var {
            Ok(o) => Ok(o),
            Err(e) => {
                if matches!(e, std::env::VarError::NotPresent) {
                    let cmp_var = option_env!("e").map(|e| e.to_string());

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
