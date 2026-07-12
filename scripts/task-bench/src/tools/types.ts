import { existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { platform } from "node:os";
import { join } from "node:path";
import type { HarnessConfig, Tool } from "../config";
import type { ProjectModel, WorkspaceModel } from "../model";

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
    /** The shared workspace model (graph, task edges, outputs). */
    model: WorkspaceModel;
    /** All generated projects, in index order (`model.projects`). */
    projects: ProjectModel[];
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
 * A snapshot of a runner's noteworthy attributes for reports — more than just
 * the resolved version, so readers can interpret the numbers in context (e.g.
 * a daemon-backed runner has a warm-run advantage; a host-provided binary
 * isn't pinned by the workspace).
 */
export interface ToolInfo {
    tool: Tool;
    /** Resolved version used in this run (pinned or detected), or null. */
    version: string | null;
    /** Whether the runner relies on a persistent background daemon. */
    daemon: boolean;
    /** How the tool is provided: an npm workspace dep, or a host binary. */
    provisioning: "workspace-dependency" | "host-binary";
    /** Semver ranges of the tool this harness supports. */
    supportedVersions: string[];
    /** One-line human-readable summary of the runner. */
    description: string;
}

/**
 * Describes the daemon behaviour for a tool adapter.
 *
 * Present on adapters that have (or historically had) a background helper
 * process.  Absent on fully in-process tools (omni, moon).
 */
export interface DaemonSupport {
    /**
     * Whether the daemon is actively used for warm runs.
     * May be false when the daemon is deprecated/optional (e.g. turbod in
     * turbo 2.x, which is no longer used for `turbo run`).
     */
    hasDaemon: boolean;
    /**
     * 'auto'   — the daemon starts as a side-effect of the first run; the
     *            bench runner does not need to start it explicitly.
     * 'manual' — must be started via startDaemon() before warm runs.
     *            startDaemon() is required in this case.
     */
    startMode: "auto" | "manual";
    /** Stop the daemon. Called before cold runs and on final cleanup. */
    stopDaemon(ctx: ToolContext): Promise<void>;
    /**
     * Explicitly start the daemon.  Required when startMode === 'manual';
     * called once after cold runs, before the priming warm run.
     */
    startDaemon?(ctx: ToolContext): Promise<void>;
    /**
     * Best-effort PIDs of daemon processes to sample alongside the CLI.
     * Only meaningful when hasDaemon is true.
     * Must never throw; return [] when the daemon cannot be located.
     */
    daemonPids?(ctx: ToolContext): Promise<number[]>;
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
    /** Semver ranges of the tool version this adapter supports. */
    supportedVersions: readonly string[];
    /**
     * One-line, human-readable summary of the runner — notable traits worth
     * knowing when reading its benchmark numbers (implementation, caching
     * model, how its daemon is controlled, etc.).
     */
    description: string;
    /**
     * Daemon configuration for tools that have (or historically had) a
     * background helper process.  Absent for fully in-process tools.
     */
    daemon?: DaemonSupport;

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
}

/** Resolve a locally-installed binary, falling back to the global name. */
export function resolveBin(rootDir: string, name: string): string {
    let binNames: string[] = [];
    if (platform() === "win32") {
        binNames = [`${name}.exe`, `${name}.cmd`, `${name}.bat`];
    } else {
        binNames = [name];
    }

    const locals = [
        ...binNames.map((bin) => join(rootDir, "node_modules", ".bin", bin)),
        ...binNames,
    ];
    for (const local of locals) {
        if (existsSync(local)) return local;
    }

    return name;
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
 * Map a shared task-dependency edge (`t{k-1}` / `^t{k}`) to moon's target
 * syntax (`~:t{k-1}` / `^:t{k}`). Other runners (omni/turbo/nx) consume the
 * shared `TaskModel.dependencies` verbatim.
 */
export function moonDependency(dep: string): string {
    return dep.startsWith("^") ? `^:${dep.slice(1)}` : `~:${dep}`;
}

export type { Tool };
