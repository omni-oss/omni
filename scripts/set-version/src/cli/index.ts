#!/usr/bin/env bun
import { Command } from "@commander-js/extra-typings";
import { BUILT_IN_PROFILES, findConfig, setVersion } from "@/index";
import { description, name, version } from "../../package.json";

const program = new Command();

program
    .name(name)
    .version(version)
    .description(description)
    .argument("<version>", "What version to set")
    .option("-d, --dir <dir>", "Directory to set version in", process.cwd())
    .option("-B, --no-built-in-profiles", "Do not use built-in profiles")
    .option("--dry-run", "Do not write changes to disk", false)
    .action(async (version, options) => {
        try {
            if (options.dryRun) {
                console.log(
                    "Dry run enabled, no changes will be written to disk",
                );
            }
            const config = await findConfig(options.dir, false);

            const profiles = options.builtInProfiles
                ? [...BUILT_IN_PROFILES, ...(config?.profiles ?? [])]
                : (config?.profiles ?? []);

            if (!profiles.length) {
                console.warn("No profiles are configured, nothing to do");
                return;
            }

            const updated = await setVersion(options.dir, version, profiles, {
                dryRun: options.dryRun,
            });

            if (updated.length) {
                console.log(`Updated ${updated.length} files:`);
                for (const file of updated) {
                    console.log(`  > ${file}`);
                }
            } else {
                console.warn("No files updated");
            }
        } catch (e) {
            console.error(e);
            process.exit(1);
        }
    });

program.parseAsync();
