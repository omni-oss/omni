use shadow_rs::{BuildPattern, ShadowBuilder};

pub fn main() -> eyre::Result<()> {
    ShadowBuilder::builder()
        .build_pattern(BuildPattern::Custom {
            if_path_changed: vec![
                "Cargo.toml".to_string(),
                "Cargo.lock".to_string(),
                "build.rs".to_string(),
            ],
            if_env_changed: vec![],
        })
        .build()?;

    Ok(())
}
