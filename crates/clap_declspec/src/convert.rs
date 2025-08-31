use clap::{Arg, Command};

use crate::{CliArg, CliCommand, CliDeclspec};

// Function to convert clap::Arg to a serializable CliArg
fn convert_arg(arg: &Arg) -> CliArg {
    CliArg {
        id: arg.get_id().to_string(),
        short: arg.get_short(),
        long: arg.get_long().map(|s| s.to_string()),
        help: arg.get_help().map(|s| s.to_string()),
        required: arg.is_required_set(),
        aliases: arg
            .get_all_aliases()
            .map(|s| s.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        default_values: arg
            .get_default_values()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>(),
        possible_values: arg
            .get_possible_values()
            .iter()
            .map(|s| s.get_name().to_string())
            .collect::<Vec<_>>(),
        env: arg.get_env().map(|s| s.to_string_lossy().to_string()),
    }
}

// Function to convert clap::Command to a serializable CliCommand
fn convert_command(cmd: &Command) -> CliCommand {
    let subcommands = cmd.get_subcommands().map(convert_command).collect();
    let aliases = cmd
        .get_all_aliases()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let author = cmd.get_author().map(ToString::to_string);

    CliCommand {
        name: cmd.get_name().to_string(),
        bin_name: cmd.get_bin_name().map(|s| s.to_string()),
        about: cmd.get_about().map(|s| s.to_string()),
        positionals: cmd.get_positionals().map(convert_arg).collect(),
        opts: cmd.get_opts().map(convert_arg).collect(),
        subcommands,
        aliases,
        author,
        version: cmd.get_version().map(|s| s.to_string()),
    }
}

fn convert_declspec(cmd: &Command) -> CliDeclspec {
    convert_command(cmd)
}

pub fn to_decspec(cmd: &Command) -> CliDeclspec {
    convert_declspec(cmd)
}
