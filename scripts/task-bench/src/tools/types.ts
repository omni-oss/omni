import { existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { join } from "node:path";
import type { HarnessConfig, Tool } from "../config";
import type { ProjectNode } from "../graph";

/** Writes a workspace-relative file (creating parent dirs) and records it. */
export type WorkspaceWriter = (
    relPath: string,
    contents: string,
) => Promise<void>;

/** Everything a tool needs to write its configuration after generation. */
export interface GenerationContext {
    /** Absolute path to the generated workspace root. */
    rootDir: string;
    config: HarnessConfig;
    /** All generated projects, in index order. */
    projects: ProjectNode[];
    /** The resolved version of this tool (pinned or detected), if known. */
    version: string | null;
    /** Write a file into the workspace. */
    write: WorkspaceWriter;
}

/** Runtime context for executing/benchmarking a tool. */
export interface ToolContext {
    /** Absolute path to the generated workspace root. */
    rootDir: string;
    /** Workspace-relative project directories, e.g. `packages/bench-p0001`. */
    projectDirs: string[];
    /** Concurrency applied identically to every runner. */
    concurrency: number;
    /** Whether the tool's persistent daemon (if any) is allowed. */
    daemon: boolean;
}

export interface RunInvocation {
    file: string;
    args: string[];
}

/**
 * A self-contained integration for one task runner. Each adapter owns:
 *   - which tool versions it supports (`supportedVersions`),
 *   - the npm dependencies it needs (`devDependencies`),
 *   - how to write its own configuration (`setup`),
 *   - and how to run / reset / clean it up at benchmark time.
 * This keeps every tool decoupled from the generator and from each other.
 */
export interface ToolAdapter {
    tool: Tool;
    /** Whether this runner has a persistent daemon that can boost warm perf. */
    hasDaemon: boolean;
    /** Semver ranges of the tool version this adapter supports. */
    supportedVersions: readonly string[];

    /** Version pinned via config for installable tools; null for external ones. */
    pinnedVersion(config: HarnessConfig): string | null;
    /** Detect the installed version of an external/global tool (e.g. omni). */
    detectVersion?(rootDir: string): Promise<string | null>;
    /** npm devDependencies to add to the root package.json. */
    devDependencies(config: HarnessConfig): Record<string, string>;
    /** Derive and write this tool's config files from the generated projects. */
    setup(ctx: GenerationContext): Promise<void>;

    /** Command that runs `task` across every project in the workspace. */
    run(task: string, ctx: ToolContext): RunInvocation;
    /** Extra environment variables for each invocation (e.g. daemon toggles). */
    env(ctx: ToolContext): Record<string, string>;
    /** Remove caches and all task outputs. */
    clearCaches(ctx: ToolContext): Promise<void>;
    /** Stop the persistent daemon, if any. Used for cold runs and cleanup. */
    stopDaemon(ctx: ToolContext): Promise<void>;
}

/** Resolve a locally-installed binary, falling back to the global name. */
export function resolveBin(rootDir: string, name: string): string {
    const local = join(rootDir, "node_modules", ".bin", name);
    return existsSync(local) ? local : name;
}

/** Remove the `dist/` output directory of every project. */
export async function removeDist(ctx: ToolContext): Promise<void> {
    await Promise.all(
        ctx.projectDirs.map((dir) =>
            rm(join(ctx.rootDir, dir, "dist"), {
                recursive: true,
                force: true,
            }),
        ),
    );
}

/**
 * Dependency edges for task index `k`, shared by omni/turbo/nx so their graphs
 * stay equivalent:
 *   - within-project: `t{k-1}` (if enabled and k > 0)
 *   - upstream:       `^t{k}`  (if enabled)
 */
export function taskDependencies(config: HarnessConfig, k: number): string[] {
    const deps: string[] = [];
    if (config.task.chainWithinProject && k > 0) deps.push(`t${k - 1}`);
    if (config.task.fanUpstream) deps.push(`^t${k}`);
    return deps;
}

/** Map upstream project indices to their names for a project's dependencies. */
export function dependencyNames(
    project: ProjectNode,
    projects: ProjectNode[],
): string[] {
    return project.dependencies.map((i) => projects[i]?.name ?? "");
}

export type { Tool };
