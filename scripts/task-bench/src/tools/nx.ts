import { execa } from "execa";
import type { HarnessConfig } from "../config";
import { type ProjectNode, taskNames } from "../graph";
import {
    type GenerationContext,
    removeDist,
    resolveBin,
    type ToolAdapter,
    type ToolContext,
    taskDependencies,
} from "./types";

export function nxRootConfig(config: HarnessConfig): string {
    const targetDefaults: Record<string, unknown> = {};
    taskNames(config).forEach((task, k) => {
        targetDefaults[task] = {
            dependsOn: taskDependencies(config, k),
            cache: true,
            outputs: [`{projectRoot}/dist/${task}.*`],
            inputs: [
                "{projectRoot}/package.json",
                "{projectRoot}/task.mjs",
                "{projectRoot}/src/**/*",
            ],
        };
    });
    return `${JSON.stringify(
        {
            $schema: "./node_modules/nx/schemas/nx-schema.json",
            targetDefaults,
        },
        null,
        2,
    )}\n`;
}

export function nxProjectConfig(
    config: HarnessConfig,
    project: ProjectNode,
): string {
    const targets: Record<string, unknown> = {};
    for (const task of taskNames(config)) {
        targets[task] = {
            executor: "nx:run-commands",
            options: { command: `node ./task.mjs ${task}`, cwd: project.dir },
        };
    }
    return `${JSON.stringify(
        {
            name: project.name,
            $schema: "../../node_modules/nx/schemas/project-schema.json",
            targets,
        },
        null,
        2,
    )}\n`;
}

export const nxAdapter: ToolAdapter = {
    tool: "nx",
    hasDaemon: true,
    supportedVersions: [">=21.0.0 <24.0.0"],

    pinnedVersion: (config) => config.versions.nx,
    devDependencies: (config) => ({ nx: config.versions.nx }),
    setup: async (ctx: GenerationContext) => {
        await ctx.write("nx.json", nxRootConfig(ctx.config));
        for (const project of ctx.projects) {
            await ctx.write(
                `${project.dir}/project.json`,
                nxProjectConfig(ctx.config, project),
            );
        }
    },

    run: (task, ctx) => ({
        file: resolveBin(ctx.rootDir, "nx"),
        args: ["run-many", "-t", task, `--parallel=${ctx.concurrency}`],
    }),
    env: (ctx) => ({
        NX_DAEMON: ctx.daemon ? "true" : "false",
        NX_TUI: "false",
    }),
    clearCaches: async (ctx: ToolContext) => {
        await removeDist(ctx);
        // nx 23 stores task results in a DB under `.nx/workspace-data` (used
        // even in no-daemon mode), which `--onlyCache` does NOT clear. A full
        // `nx reset` is the only reliable way to force a genuinely cold run.
        // clearCaches is only invoked for cold runs, so tearing down the daemon
        // too is acceptable (warm runs never call this).
        await execa(resolveBin(ctx.rootDir, "nx"), ["reset"], {
            cwd: ctx.rootDir,
            reject: false,
            stdio: "ignore",
            env: { NX_DAEMON: ctx.daemon ? "true" : "false", NX_TUI: "false" },
        });
    },
    stopDaemon: async (ctx: ToolContext) => {
        await execa(resolveBin(ctx.rootDir, "nx"), ["reset"], {
            cwd: ctx.rootDir,
            reject: false,
            stdio: "ignore",
            env: { NX_DAEMON: "false" },
        });
    },
};
