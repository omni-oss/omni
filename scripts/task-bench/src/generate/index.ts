import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { execa } from "execa";
import {
    type HarnessConfig,
    type HarnessConfigInput,
    resolveConfig,
} from "../config";
import { buildGraph, type ProjectNode, taskNames } from "../graph";
import { getAdapters, resolveToolVersions, type ToolAdapter } from "../tools";
import { sourceFile, taskRunner } from "./templates";

export interface GenerateResult {
    rootDir: string;
    config: HarnessConfig;
    projects: ProjectNode[];
    /** Every file written, workspace-relative. */
    files: string[];
}

async function writeText(
    rootDir: string,
    relPath: string,
    contents: string,
    written: string[],
): Promise<void> {
    const abs = join(rootDir, relPath);
    await mkdir(dirname(abs), { recursive: true });
    await writeFile(abs, contents);
    written.push(relPath);
}

function rootPackageJson(
    config: HarnessConfig,
    adapters: ToolAdapter[],
): string {
    // Each tool contributes its own npm dependencies (decoupled).
    const devDependencies: Record<string, string> = {};
    for (const adapter of adapters) {
        Object.assign(devDependencies, adapter.devDependencies(config));
    }

    return `${JSON.stringify(
        {
            name: "task-bench-harness",
            private: true,
            packageManager: `bun@${config.versions.bun}`,
            workspaces: ["packages/*"],
            ...(Object.keys(devDependencies).length ? { devDependencies } : {}),
        },
        null,
        2,
    )}\n`;
}

function projectPackageJson(
    config: HarnessConfig,
    project: ProjectNode,
    projects: ProjectNode[],
): string {
    const scripts: Record<string, string> = {};
    for (const task of taskNames(config)) {
        scripts[task] = `node ./task.mjs ${task}`;
    }
    const dependencies: Record<string, string> = {};
    for (const depIndex of project.dependencies) {
        const dep = projects[depIndex];
        if (dep) dependencies[dep.name] = "workspace:*";
    }
    return `${JSON.stringify(
        {
            name: project.name,
            version: "0.0.0",
            private: true,
            scripts,
            ...(Object.keys(dependencies).length ? { dependencies } : {}),
        },
        null,
        2,
    )}\n`;
}

function gitignore(): string {
    return [
        "node_modules",
        "dist",
        ".turbo",
        ".nx",
        ".omni",
        ".moon/cache",
        "*.log",
        "",
    ].join("\n");
}

function readme(config: HarnessConfig): string {
    return [
        "# task-bench harness",
        "",
        "Auto-generated benchmark workspace. Do not edit by hand; regenerate via",
        "`@omni-oss/task-bench` instead.",
        "",
        `- projects: **${config.projects}**`,
        `- tasks per project: **${config.tasksPerProject}**`,
        `- dependency strategy: **${config.dependency.strategy}**`,
        `- tools: **${config.tools.join(", ")}**`,
        "",
        "The exact configuration is captured in `bench.config.json`.",
        "",
    ].join("\n");
}

/**
 * moon only enables its cache when the workspace is a git repository, so
 * initialize + commit one. This is harmless (and realistic) for the other
 * runners. Failures (e.g. git missing) are ignored.
 */
async function initGitRepo(rootDir: string): Promise<void> {
    const opts = { cwd: rootDir, reject: false, stdio: "ignore" as const };
    await execa("git", ["init", "-q"], opts);
    await execa("git", ["add", "-A"], opts);
    await execa(
        "git",
        [
            "-c",
            "user.email=bench@task-bench.local",
            "-c",
            "user.name=task-bench",
            "commit",
            "-qm",
            "generated benchmark workspace",
        ],
        opts,
    );
}

/**
 * Generate a complete benchmark workspace at `rootDir`. Existing contents at
 * `rootDir` are removed first so the workspace is reproducible.
 *
 * The generator only produces the neutral, tool-agnostic workspace (projects,
 * sources, task runners). Each enabled tool then writes its own configuration
 * via its adapter, keeping the tools fully decoupled.
 */
export async function generateWorkspace(
    rootDir: string,
    input?: HarnessConfigInput,
): Promise<GenerateResult> {
    const config = resolveConfig(input);
    const projects = buildGraph(config);
    const written: string[] = [];
    const adapters = getAdapters(config.tools);

    await rm(rootDir, { recursive: true, force: true });
    await mkdir(rootDir, { recursive: true });

    // Validate every enabled tool's version up-front (fails fast).
    const versions = await resolveToolVersions(config, rootDir);

    // Neutral, tool-agnostic workspace.
    await writeText(
        rootDir,
        "package.json",
        rootPackageJson(config, adapters),
        written,
    );
    await writeText(rootDir, ".gitignore", gitignore(), written);
    await writeText(rootDir, "README.md", readme(config), written);
    await writeText(
        rootDir,
        "bench.config.json",
        `${JSON.stringify(config, null, 2)}\n`,
        written,
    );

    for (const project of projects) {
        const dir = project.dir;
        await writeText(
            rootDir,
            `${dir}/package.json`,
            projectPackageJson(config, project, projects),
            written,
        );
        await writeText(
            rootDir,
            `${dir}/src/index.js`,
            sourceFile(project),
            written,
        );
        await writeText(
            rootDir,
            `${dir}/task.mjs`,
            taskRunner(config, project),
            written,
        );
    }

    // Let each tool derive and write its own configuration.
    const write = (relPath: string, contents: string) =>
        writeText(rootDir, relPath, contents, written);
    for (const adapter of adapters) {
        await adapter.setup({
            rootDir,
            config,
            projects,
            version: versions.get(adapter.tool) ?? null,
            write,
        });
    }

    // moon requires a git repo to enable its cache.
    await initGitRepo(rootDir);

    return { rootDir, config, projects, files: written };
}
