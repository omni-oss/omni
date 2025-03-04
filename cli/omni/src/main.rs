use deno_core::{JsRuntime, RuntimeOptions};

fn main() -> eyre::Result<()> {
    let mut rt = JsRuntime::new(RuntimeOptions::default());

    rt.execute_script("test", "let x1 = 'Hello, world'; console.log(x1);")?;

    Ok(())
}
