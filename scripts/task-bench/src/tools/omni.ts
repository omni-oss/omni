import { rm } from "node:fs/promises";
import { join } from "node:path";
import { execa } from "execa";
import { type OmniRenderOptions, renderOmni } from "../model";
import {
    type GenerationContext,
    removeDist,
    type ToolAdapter,
    type ToolContext,
} from "./types";

/**
 * How omni tasks are invoked in the generated workspace and which files feed a
 * project's cache key. Passed to the shared `renderOmni` core so the omni layer
 * is generated identically to the in-process Rust bench (only these host-
 * specific bits differ: the launcher/`task.mjs` command and the neutral-base
 * cache inputs).
 */
export const OMNI_RENDER_OPTIONS: OmniRenderOptions = {
    taskCommandTemplate: "node ./task.mjs {task_id}",
    projectCacheKeyFiles: ["package.json", "task.mjs", "src/**/*.js"],
};

export const omniAdapter: ToolAdapter = {
    tool: "omni",
    hasDaemon: false,
    // omni is a host-provided binary, not installed per-workspace.
    supportedVersions: [">=0.16.0"],
    description:
        "The Rust task runner under test. Host-provided binary (not pinned by the workspace); no persistent daemon, so every run is a fresh process.",

    pinnedVersion: () => null,
    detectVersion: async (rootDir) => {
        const result = await execa("omni", ["--version"], {
            cwd: rootDir,
            reject: false,
        });
        const match = `${result.stdout} ${result.stderr}`.match(
            /(\d+\.\d+\.\d+[\w.-]*)/,
        );
        return match?.[1] ?? null;
    },
    devDependencies: () => ({}),
    setup: async (ctx: GenerationContext) => {
        // The omni layer (workspace + per-project configs) is rendered by the
        // shared Rust core, so it can never drift from the in-process bench.
        for (const [relPath, contents] of renderOmni(
            ctx.model,
            OMNI_RENDER_OPTIONS,
        )) {
            await ctx.write(relPath, contents);
        }
    },

    run: (task, ctx) => ({
        file: "omni",
        args: ["run", task, "-u", "stream", "-c", String(ctx.concurrency)],
    }),
    env: () => ({}),
    clearCaches: async (ctx: ToolContext) => {
        await removeDist(ctx);
        await rm(join(ctx.rootDir, ".omni", "cache"), {
            recursive: true,
            force: true,
        });
    },
    stopDaemon: async () => {
        // omni has no persistent daemon.
    },
};
