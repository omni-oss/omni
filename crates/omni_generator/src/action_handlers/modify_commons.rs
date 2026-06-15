use std::{borrow::Cow, path::Path};

use omni_generator_configurations::{
    CommonModifyConfiguration, ModifyInlineContentEntry,
};

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{augment_tera_context, get_target_file},
    },
    error::{Error, ErrorInner},
};

pub async fn modify_one<'a>(
    entries: &'a [ModifyInlineContentEntry],
    common: &'a CommonModifyConfiguration,
    ctx: &HandlerContext<'a>,
    sys: &'a impl GeneratorSys,
) -> Result<(), Error> {
    let target_name = &common.target;
    let target =
        get_target_file(target_name, ctx, ctx.input_provider, sys).await?;

    let tera_ctx_with_data =
        augment_tera_context(ctx.tera_context_values, Some(&common.data))?;

    let target = omni_tera::one_off(
        &target.to_string_lossy(),
        "output_path",
        &tera_ctx_with_data,
    )?;

    let mut content = sys
        .fs_read_to_string_async(&target)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ErrorInner::new_file_not_found(
                    Path::new(&target).to_path_buf(),
                    e,
                )
            } else {
                ErrorInner::new_generic_io(e)
            }
        })?
        .into_owned();

    for entry in entries {
        let rg = regex::Regex::new(&entry.pattern)?;
        if !rg.is_match(&content) {
            return Err(Error::custom(format!(
                "pattern '{}' not found in template for action {}",
                &entry.pattern, ctx.resolved_action_name
            )));
        }

        let rendered = if common.render {
            Cow::Owned(omni_tera::one_off(
                &entry.content,
                format!("template for action {}", ctx.resolved_action_name),
                &tera_ctx_with_data,
            )?)
        } else {
            Cow::Borrowed(&entry.content)
        };

        content = rg.replace_all(&content, &rendered[..]).into_owned();
    }

    sys.fs_write_async(&target, &content).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use maps::UnorderedMap;
    use omni_generator_configurations::{
        CommonModifyConfiguration, ModifyInlineContentEntry,
    };
    use system_traits::impls::RealSys;

    use super::super::test_harness::Fixture;
    use super::modify_one;

    #[tokio::test]
    async fn replaces_all_occurrences_of_pattern() {
        let fix = Fixture::new().with_output_target("src", "target.txt");
        std::fs::write(fix.output.path().join("target.txt"), "foo bar foo")
            .unwrap();

        let entries = [ModifyInlineContentEntry {
            pattern: "foo".to_string(),
            content: "qux".to_string(),
        }];
        let common = CommonModifyConfiguration {
            target: "src".to_string(),
            data: UnorderedMap::default(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        modify_one(&entries, &common, &ctx, &sys).await.unwrap();

        let result =
            std::fs::read_to_string(fix.output.path().join("target.txt"))
                .unwrap();
        assert_eq!(result, "qux bar qux");
    }

    #[tokio::test]
    async fn renders_tera_replacement() {
        let fix = Fixture::new()
            .with_value("name", "Alice")
            .with_output_target("src", "t.txt");
        std::fs::write(fix.output.path().join("t.txt"), "OLD").unwrap();

        let entries = [ModifyInlineContentEntry {
            pattern: "OLD".to_string(),
            content: "{{ name }}".to_string(),
        }];
        let common = CommonModifyConfiguration {
            target: "src".to_string(),
            data: UnorderedMap::default(),
            render: true,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        modify_one(&entries, &common, &ctx, &sys).await.unwrap();

        let result =
            std::fs::read_to_string(fix.output.path().join("t.txt")).unwrap();
        assert_eq!(result, "Alice");
    }

    #[tokio::test]
    async fn returns_error_when_pattern_not_found() {
        let fix = Fixture::new().with_output_target("src", "target.txt");
        std::fs::write(fix.output.path().join("target.txt"), "hello world")
            .unwrap();

        let entries = [ModifyInlineContentEntry {
            pattern: "NOTFOUND".to_string(),
            content: "x".to_string(),
        }];
        let common = CommonModifyConfiguration {
            target: "src".to_string(),
            data: UnorderedMap::default(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        let result = modify_one(&entries, &common, &ctx, &sys).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn returns_error_for_missing_target_file() {
        let fix =
            Fixture::new().with_output_target("src", "does_not_exist.txt");
        // Don't pre-write the file

        let entries = [ModifyInlineContentEntry {
            pattern: "NOTFOUND".to_string(),
            content: "x".to_string(),
        }];
        let common = CommonModifyConfiguration {
            target: "src".to_string(),
            data: UnorderedMap::default(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        let result = modify_one(&entries, &common, &ctx, &sys).await;
        assert!(result.is_err());
    }
}
