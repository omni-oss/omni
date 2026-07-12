import { rm } from "node:fs/promises";
import { join } from "node:path";
import { stringify as stringifyYaml } from "yaml";
import type { ProjectModel } from "../model";
import {
    type GenerationContext,
    moonDependency,
    removeDist,
    resolveBin,
    type ToolAdapter,
    type ToolContext,
} from "./types";

const WORKSPACE_SCHEMA = "https://moonrepo.dev/schemas/workspace.json";
const TOOLCHAIN_SCHEMA = "https://moonrepo.dev/schemas/toolchain.json";
const PROJECT_SCHEMA = "https://moonrepo.dev/schemas/project.json";

export function moonWorkspaceConfig(): string {
    return stringifyYaml({
        $schema: WORKSPACE_SCHEMA,
        projects: ["packages/*"],
        // Pin the default branch so moon's CI touched-files detection has a
        // stable base (the generated repo is initialized on `master`).
        vcs: { defaultBranch: "master" },
    });
}

export function moonToolchainConfig(): string {
    // Intentionally empty: no managed toolchain, so moon runs `node` from the
    // system PATH (kept equivalent to the other runners).
    return stringifyYaml({ $schema: TOOLCHAIN_SCHEMA });
}

export function moonProjectConfig(project: ProjectModel): string {
    const tasks: Record<string, unknown> = {};
    for (const task of project.tasks) {
        const deps = task.dependencies.map(moonDependency);
        tasks[task.name] = {
            command: `node ./task.mjs ${task.name}`,
            ...(deps.length ? { deps } : {}),
            inputs: ["package.json", "task.mjs", "src/**/*"],
            outputs: task.outputGlobs,
        };
    }

    const doc: Record<string, unknown> = {
        $schema: PROJECT_SCHEMA,
        id: project.name,
        layer: "library",
        language: "javascript",
        ...(project.dependencies.length
            ? { dependsOn: project.dependencies }
            : {}),
        tasks,
    };

    return stringifyYaml(doc);
}

export const moonAdapter: ToolAdapter = {
    tool: "moon",
    supportedVersions: ["^2.0.0"],
    description:
        "moonrepo. No persistent daemon — every invocation is a fresh process; installed as a workspace devDependency (@moonrepo/cli) and requires a git repo to enable caching.",

    pinnedVersion: (config) => config.versions.moon,
    devDependencies: (config) => ({ "@moonrepo/cli": config.versions.moon }),
    setup: async (ctx: GenerationContext) => {
        await ctx.write(".moon/workspace.yml", moonWorkspaceConfig());
        await ctx.write(".moon/toolchain.yml", moonToolchainConfig());
        for (const project of ctx.projects) {
            await ctx.write(
                `${project.dir}/moon.yml`,
                moonProjectConfig(project),
            );
        }
    },

    run: (task, ctx) => ({
        file: resolveBin(ctx.rootDir, "moon"),
        args: ["run", `:${task}`, "--concurrency", String(ctx.concurrency)],
    }),
    env: () => ({}),
    clearCaches: async (ctx: ToolContext) => {
        await removeDist(ctx);
        await rm(join(ctx.rootDir, ".moon", "cache"), {
            recursive: true,
            force: true,
        });
    },
};
