use std::{borrow::Cow, path::Path};

use omni_generator_configurations::{
    CommonInsertConfiguration, InsertInlineContentEntry,
};
use omni_messages::GeneratorEventSubscriber;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{augment_tera_context, map_file_io_error, resolve_target_file},
    },
    error::{Error, ErrorInner},
};

pub async fn insert_one<'a, S: GeneratorEventSubscriber>(
    entries: &[InsertInlineContentEntry],
    prepend: bool,
    common: &'a CommonInsertConfiguration,
    ctx: &HandlerContext<'a, S>,
    sys: &'a impl GeneratorSys,
) -> Result<(), Error> {
    let target_name = &common.target;
    let target =
        resolve_target_file(target_name, ctx, ctx.input_provider, sys).await?;

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
        .map_err(|e| map_file_io_error(Path::new(&target), e))?
        .into_owned();

    for entry in entries {
        let rg = regex::Regex::new(&entry.pattern)?;
        if !rg.is_match(&content) {
            return Err(Error::custom(format!(
                "pattern '{}' not found in template for action {}",
                &entry.pattern, ctx.resolved_action_name
            )));
        }

        let rendered: Cow<str> = if common.render {
            Cow::Owned(omni_tera::one_off(
                &entry.content,
                format!("template for action {}", ctx.resolved_action_name),
                &tera_ctx_with_data,
            )?)
        } else {
            Cow::Borrowed(&entry.content)
        };

        let mut stop_inserting = false;
        let mut file: Vec<&str> = vec![];
        for line in content.split(&common.separator) {
            let matching = rg.is_match(line);
            if !stop_inserting && prepend && matching {
                file.push(&rendered);
                if common.unique {
                    stop_inserting = true;
                }
            }

            file.push(&line);

            if !stop_inserting && !prepend && matching {
                file.push(&rendered);
                stop_inserting = true;
                if common.unique {
                    stop_inserting = true;
                }
            }
        }
        content = file.join(&common.separator);
    }

    sys.fs_write_async(&target, content).await.map_err(|e| {
        ErrorInner::new_failed_to_write_file(Path::new(&target), e)
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use maps::UnorderedMap;
    use omni_generator_configurations::{
        CommonInsertConfiguration, InsertInlineContentEntry,
    };
    use system_traits::impls::RealSys;

    use super::super::test_harness::Fixture;
    use super::insert_one;

    #[tokio::test]
    async fn appends_content_after_first_matching_line() {
        let fix = Fixture::new().with_output_target("src", "src.txt");
        std::fs::write(
            fix.output.path().join("src.txt"),
            "line1\nMARKER\nline3",
        )
        .unwrap();

        let entries = [InsertInlineContentEntry {
            pattern: "MARKER".to_string(),
            content: "inserted".to_string(),
        }];
        let common = CommonInsertConfiguration {
            separator: "\n".to_string(),
            unique: true,
            data: UnorderedMap::default(),
            target: "src".to_string(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        insert_one(&entries, false, &common, &ctx, &sys)
            .await
            .unwrap();

        let result =
            std::fs::read_to_string(fix.output.path().join("src.txt")).unwrap();
        assert_eq!(result, "line1\nMARKER\ninserted\nline3");
    }

    #[tokio::test]
    async fn prepends_content_before_first_matching_line() {
        let fix = Fixture::new().with_output_target("src", "src.txt");
        std::fs::write(
            fix.output.path().join("src.txt"),
            "line1\nMARKER\nline3",
        )
        .unwrap();

        let entries = [InsertInlineContentEntry {
            pattern: "MARKER".to_string(),
            content: "inserted".to_string(),
        }];
        let common = CommonInsertConfiguration {
            separator: "\n".to_string(),
            unique: true,
            data: UnorderedMap::default(),
            target: "src".to_string(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        insert_one(&entries, true, &common, &ctx, &sys)
            .await
            .unwrap();

        let result =
            std::fs::read_to_string(fix.output.path().join("src.txt")).unwrap();
        assert_eq!(result, "line1\ninserted\nMARKER\nline3");
    }

    #[tokio::test]
    async fn non_unique_prepend_inserts_before_every_match() {
        let fix = Fixture::new().with_output_target("src", "src.txt");
        std::fs::write(
            fix.output.path().join("src.txt"),
            "MARKER\nstuff\nMARKER",
        )
        .unwrap();

        let entries = [InsertInlineContentEntry {
            pattern: "MARKER".to_string(),
            content: "X".to_string(),
        }];
        let common = CommonInsertConfiguration {
            separator: "\n".to_string(),
            unique: false,
            data: UnorderedMap::default(),
            target: "src".to_string(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        insert_one(&entries, true, &common, &ctx, &sys)
            .await
            .unwrap();

        let result =
            std::fs::read_to_string(fix.output.path().join("src.txt")).unwrap();
        assert_eq!(result, "X\nMARKER\nstuff\nX\nMARKER");
    }

    #[tokio::test]
    async fn renders_tera_content_before_inserting() {
        let fix = Fixture::new()
            .with_value("name", "Alice")
            .with_output_target("src", "src.txt");
        std::fs::write(
            fix.output.path().join("src.txt"),
            "before\nMARKER\nafter",
        )
        .unwrap();

        let entries = [InsertInlineContentEntry {
            pattern: "MARKER".to_string(),
            content: "Hello {{ name }}".to_string(),
        }];
        let common = CommonInsertConfiguration {
            separator: "\n".to_string(),
            unique: true,
            data: UnorderedMap::default(),
            target: "src".to_string(),
            render: true,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        insert_one(&entries, false, &common, &ctx, &sys)
            .await
            .unwrap();

        let result =
            std::fs::read_to_string(fix.output.path().join("src.txt")).unwrap();
        assert_eq!(result, "before\nMARKER\nHello Alice\nafter");
    }

    #[tokio::test]
    async fn returns_error_when_pattern_not_found() {
        let fix = Fixture::new().with_output_target("src", "src.txt");
        std::fs::write(fix.output.path().join("src.txt"), "line1\nline2")
            .unwrap();

        let entries = [InsertInlineContentEntry {
            pattern: "NOTFOUND".to_string(),
            content: "x".to_string(),
        }];
        let common = CommonInsertConfiguration {
            separator: "\n".to_string(),
            unique: true,
            data: UnorderedMap::default(),
            target: "src".to_string(),
            render: false,
        };
        let ctx = fix.ctx();
        let sys = RealSys;

        let result = insert_one(&entries, false, &common, &ctx, &sys).await;
        assert!(result.is_err());
    }
}
