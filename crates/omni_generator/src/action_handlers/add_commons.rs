use std::{borrow::Cow, path::Path};

use omni_messages::GeneratorEventSubscriber;
use omni_generator_configurations::CommonAddConfiguration;

use crate::{
    GeneratorSys,
    action_handlers::{
        HandlerContext,
        utils::{
            augment_tera_context, ensure_dir_exists, get_output_path, overwrite,
        },
    },
    error::{Error, ErrorInner},
};

pub async fn add_one<'a, TRender, TSys, S: GeneratorEventSubscriber>(
    file: &'a Path,
    content: &'a str,
    base_path: Option<&'a Path>,
    render: TRender,
    common: &CommonAddConfiguration,
    flatten: bool,
    ctx: &HandlerContext<'a, S>,
    sys: &'a TSys,
) -> Result<(), Error>
where
    TRender: FnOnce(&str, &omni_tera::Context) -> omni_tera::Result<String>
        + Send
        + Sync
        + 'a,
    TSys: GeneratorSys,
{
    let output_path = get_output_path(
        common.target.as_deref(),
        &file,
        base_path,
        ctx,
        ctx.input_provider,
        if common.render { &["tpl"] } else { &[] },
        flatten,
        ctx.gen_session,
        sys,
    )
    .await?;

    let expanded_output = omni_tera::one_off(
        &output_path.to_string_lossy(),
        "output_path",
        ctx.tera_context_values,
    )?;

    let output_path = Path::new(&expanded_output);
    if let Some(did_overwrite) = overwrite(
        &output_path,
        ctx.overwrite.or(common.overwrite),
        ctx.input_provider,
        sys,
    )
    .await?
        && !did_overwrite
    {
        log::info!("Skipped writing to path {}", output_path.display());
        return Ok(());
    }

    ensure_dir_exists(&output_path.parent().expect("should have parent"), sys)
        .await?;

    let tera_ctx_with_data =
        augment_tera_context(ctx.tera_context_values, Some(&common.data))?;

    let result = if common.render {
        Cow::Owned(render(content, &tera_ctx_with_data)?)
    } else {
        Cow::Borrowed(content)
    };

    sys.fs_write_async(&output_path, result.as_bytes())
        .await
        .map_err(|e| ErrorInner::new_failed_to_write_file(&output_path, e))?;

    log::info!("Wrote to path {}", output_path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use maps::UnorderedMap;
    use omni_generator_configurations::{
        CommonAddConfiguration, OverwriteConfiguration,
    };
    use system_traits::impls::RealSys;

    use super::super::test_harness::Fixture;
    use super::add_one;

    fn common(render: bool) -> CommonAddConfiguration {
        CommonAddConfiguration {
            overwrite: None,
            target: None,
            data: UnorderedMap::default(),
            render,
        }
    }

    #[tokio::test]
    async fn writes_content_verbatim() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let sys = RealSys;
        add_one(
            Path::new("out.txt"),
            "hello world",
            None,
            |_s, _ctx| unreachable!(),
            &common(false),
            false,
            &ctx,
            &sys,
        )
        .await
        .unwrap();
        let contents =
            std::fs::read_to_string(fix.output.path().join("out.txt")).unwrap();
        assert_eq!(contents, "hello world");
    }

    #[tokio::test]
    async fn renders_tera_template() {
        let fix = Fixture::new().with_value("name", "Alice");
        let ctx = fix.ctx();
        let sys = RealSys;
        add_one(
            Path::new("out.txt"),
            "Hello {{ name }}",
            None,
            |s, ctx| omni_tera::one_off(s, "tpl", ctx),
            &common(true),
            false,
            &ctx,
            &sys,
        )
        .await
        .unwrap();
        let contents =
            std::fs::read_to_string(fix.output.path().join("out.txt")).unwrap();
        assert_eq!(contents, "Hello Alice");
    }

    #[tokio::test]
    async fn creates_nested_directories() {
        let fix = Fixture::new();
        let ctx = fix.ctx();
        let sys = RealSys;
        add_one(
            Path::new("sub/nested/out.txt"),
            "deep",
            None,
            |_s, _ctx| unreachable!(),
            &common(false),
            false,
            &ctx,
            &sys,
        )
        .await
        .unwrap();
        let contents = std::fs::read_to_string(
            fix.output.path().join("sub/nested/out.txt"),
        )
        .unwrap();
        assert_eq!(contents, "deep");
    }

    #[tokio::test]
    async fn skips_write_when_file_exists_and_overwrite_never() {
        let fix = Fixture::new().with_overwrite(OverwriteConfiguration::Never);
        std::fs::write(fix.output.path().join("out.txt"), "original").unwrap();
        let ctx = fix.ctx();
        let sys = RealSys;
        add_one(
            Path::new("out.txt"),
            "new content",
            None,
            |_s, _ctx| unreachable!(),
            &common(false),
            false,
            &ctx,
            &sys,
        )
        .await
        .unwrap();
        let contents =
            std::fs::read_to_string(fix.output.path().join("out.txt")).unwrap();
        assert_eq!(contents, "original");
    }

    #[tokio::test]
    async fn overwrites_file_when_configured_always() {
        let fix = Fixture::new().with_overwrite(OverwriteConfiguration::Always);
        std::fs::write(fix.output.path().join("out.txt"), "original").unwrap();
        let ctx = fix.ctx();
        let sys = RealSys;
        add_one(
            Path::new("out.txt"),
            "replaced",
            None,
            |_s, _ctx| unreachable!(),
            &common(false),
            false,
            &ctx,
            &sys,
        )
        .await
        .unwrap();
        let contents =
            std::fs::read_to_string(fix.output.path().join("out.txt")).unwrap();
        assert_eq!(contents, "replaced");
    }
}
