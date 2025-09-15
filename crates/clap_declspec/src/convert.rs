use std::collections::{HashMap, hash_map::Entry};

use clap::{Arg, Command};

use crate::{CliArg, CliArgLink, CliCommand, CliDeclspec, CliGroup};

// Function to convert clap::Arg to a serializable CliArg
fn convert_arg(
    arg: &Arg,
    groups: Vec<String>,
    conflicts_with: Vec<CliArgLink>,
) -> CliArg {
    CliArg {
        groups,
        conflicts_with,
        id: arg.get_id().to_string(),
        short: arg.get_short(),
        long: arg.get_long().map(|s| s.to_string()),
        help: arg.get_help().map(|s| s.to_string()),
        long_help: arg.get_long_help().map(|s| s.to_string()),
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

fn convert_group(group: &clap::ArgGroup) -> CliGroup {
    CliGroup {
        id: group.get_id().to_string(),
        arg_ids: group.get_args().map(|s| s.to_string()).collect(),
    }
}

// Function to convert clap::Command to a serializable CliCommand
fn convert_command(cmd: &Command) -> CliCommand {
    let subcommands = cmd.get_subcommands().map(convert_command).collect();
    let aliases = cmd
        .get_all_aliases()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let groups = cmd.get_groups().map(convert_group).collect::<Vec<_>>();
    let mut group_mapping = HashMap::<String, Vec<String>>::new();
    for group in &groups {
        for arg_id in &group.arg_ids {
            let entry = group_mapping.entry(arg_id.to_string());
            match entry {
                Entry::Occupied(occupied_entry) => {
                    occupied_entry.into_mut().push(group.id.clone());
                }
                Entry::Vacant(vacant_entry) => {
                    vacant_entry.insert(vec![group.id.clone()]);
                }
            }
        }
    }

    let mut positionals = vec![];
    let mut opts = vec![];

    for arg in cmd.get_arguments() {
        let arg_id = arg.get_id().to_string();
        let mut conflicts_with = vec![];

        for conflict in cmd.get_arg_conflicts_with(arg) {
            conflicts_with.push(convert_arg_link(conflict));
        }

        let groups = group_mapping.remove(&arg_id).unwrap_or_default();

        if arg.is_positional() {
            positionals.push(convert_arg(arg, groups, conflicts_with));
        } else {
            opts.push(convert_arg(arg, groups, conflicts_with));
        }
    }

    CliCommand {
        name: cmd.get_name().to_string(),
        bin_name: cmd.get_bin_name().map(|s| s.to_string()),
        about: cmd.get_about().map(|s| s.to_string()),
        long_about: cmd.get_long_about().map(|s| s.to_string()),
        positionals,
        opts,
        groups,
        subcommands,
        aliases,
        author: cmd.get_author().map(|s| s.to_string()),
        version: cmd.get_version().map(|s| s.to_string()),
        long_version: cmd.get_long_version().map(|s| s.to_string()),
        long_flag: cmd.get_long_flag().map(|s| s.to_string()),
        short_flag: cmd.get_short_flag(),
    }
}

fn convert_arg_link(arg: &clap::Arg) -> CliArgLink {
    CliArgLink {
        id: arg.get_id().to_string(),
        long: arg.get_long().map(|s| s.to_string()),
        short: arg.get_short(),
    }
}

fn convert_declspec(cmd: &Command) -> CliDeclspec {
    convert_command(cmd)
}

pub fn to_decspec(cmd: &Command) -> CliDeclspec {
    convert_declspec(cmd)
}
