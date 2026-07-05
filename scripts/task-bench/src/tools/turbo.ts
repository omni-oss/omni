import { rm } from "node:fs/promises";
import { join } from "node:path";
import { execa } from "execa";
import type { HarnessConfig } from "../config";
import { taskNames } from "../graph";
import {
    type GenerationContext,
    removeDist,
    resolveBin,
    type ToolAdapter,
    type ToolContext,
    taskDependencies,
} from "./types";

export function turboRootConfig(config: HarnessConfig): string {
    const tasks: Record<string, unknown> = {};
    taskNames(config).forEach((task, k) => {
        tasks[task] = {
            dependsOn: taskDependencies(config, k),
            outputs: [`dist/${task}.*`],
            inputs: ["package.json", "task.mjs", "src/**"],
        };
    });
    return `${JSON.stringify(
        {
            $schema: "https://turbo.build/schema.json",
            // Turbo 2.x defaults to strict env mode, which would strip the
            // benchmark's execution-marker var. Pass it through (not hashed)
            // so cache-hit verification works without perturbing cache keys.
            globalPassThroughEnv: ["TASK_BENCH_EXEC_LOG"],
            tasks,
        },
        null,
        2,
    )}\n`;
}

export const turboAdapter: ToolAdapter = {
    tool: "turbo",
    hasDaemon: true,
    supportedVersions: ["^2.0.0"],

    pinnedVersion: (config) => config.versions.turbo,
    devDependencies: (config) => ({ turbo: config.versions.turbo }),
    setup: async (ctx: GenerationContext) => {
        await ctx.write("turbo.json", turboRootConfig(ctx.config));
    },

    run: (task, ctx) => ({
        file: resolveBin(ctx.rootDir, "turbo"),
        args: [
            "run",
            task,
            "--log-order=stream",
            `--concurrency=${ctx.concurrency}`,
            ctx.daemon ? "--daemon" : "--no-daemon",
        ],
    }),
    env: () => ({}),
    clearCaches: async (ctx: ToolContext) => {
        await removeDist(ctx);
        await rm(join(ctx.rootDir, ".turbo"), {
            recursive: true,
            force: true,
        });
        await rm(join(ctx.rootDir, "node_modules", ".cache", "turbo"), {
            recursive: true,
            force: true,
        });
    },
    stopDaemon: async (ctx: ToolContext) => {
        await execa(resolveBin(ctx.rootDir, "turbo"), ["daemon", "stop"], {
            cwd: ctx.rootDir,
            reject: false,
            stdio: "ignore",
        });
    },
};
