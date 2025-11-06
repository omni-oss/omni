use std::path::Path;

use derive_new::new;
use maps::UnorderedMap;
use omni_generator_configurations::ActionConfiguration;
use value_bag::OwnedValueBag;

use crate::error::Error;

#[derive(Debug, new)]
pub struct ExecuteActionsArgs<'a> {
    pub dry_run: bool,
    pub output_dir: &'a Path,
    pub actions: &'a [ActionConfiguration],
    pub context_values: &'a UnorderedMap<String, OwnedValueBag>,
}

pub async fn execute_actions<'a>(
    args: &ExecuteActionsArgs<'a>,
) -> Result<(), Error> {
    trace::info!(
        "Executing actions: \ndry_run={}\noutput_dir={}\nactions={:#?}\ncontext_values={:#?}",
        args.dry_run,
        args.output_dir.display(),
        args.actions,
        args.context_values
    );
    Ok(())
}
