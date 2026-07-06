import { rm } from "node:fs/promises";
import { join } from "node:path";
import { execa } from "execa";
import { stringify as stringifyYaml } from "yaml";
import type { HarnessConfig } from "../config";
import { type ProjectNode, taskNames } from "../graph";
import {
    dependencyNames,
    type GenerationContext,
    removeDist,
    type ToolAdapter,
    type ToolContext,
    taskDependencies,
} from "./types";

const SCHEMA =
    "# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/project.json";
const WORKSPACE_SCHEMA =
    "# yaml-language-server: $schema=https://raw.githubusercontent.com/omni-oss/json-schemas/refs/heads/main/workspace.json";

export function omniWorkspaceConfig(): string {
    const body = stringifyYaml({ ui: "stream", projects: ["packages/*"] });
    return `${WORKSPACE_SCHEMA}\n${body}`;
}

export function omniProjectConfig(
    config: HarnessConfig,
    project: ProjectNode,
    projects: ProjectNode[],
): string {
    const tasks: Record<string, unknown> = {};
    taskNames(config).forEach((task, k) => {
        const deps = taskDependencies(config, k);
        tasks[task] = {
            exec: `node ./task.mjs ${task}`,
            ...(deps.length ? { dependencies: deps } : {}),
            cache: { output: { files: [`dist/${task}.*`] } },
        };
    });

    const doc: Record<string, unknown> = {
        name: project.name,
        ...(project.dependencies.length
            ? { dependencies: dependencyNames(project, projects) }
            : {}),
        cache: { key: { files: ["package.json", "task.mjs", "src/**/*.js"] } },
        tasks,
    };

    return `${SCHEMA}\n${stringifyYaml(doc)}`;
}

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
        await ctx.write("workspace.omni.yaml", omniWorkspaceConfig());
        for (const project of ctx.projects) {
            await ctx.write(
                `${project.dir}/project.omni.yaml`,
                omniProjectConfig(ctx.config, project, ctx.projects),
            );
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
