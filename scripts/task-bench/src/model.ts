import {
    buildModel as buildModelWasm,
    modelVersion as modelVersionWasm,
    renderOmni as renderOmniWasm,
} from "@omni-oss/workspace-gen";
import type { HarnessConfig } from "./config";

/** A single task within a project (mirrors the core `TaskModel`). */
export interface TaskModel {
    name: string;
    /** Resolved edges: intra-project (`t{k-1}`) then upstream (`^t{k}`). */
    dependencies: string[];
    /** Declared cache output globs, e.g. `["dist/t1.*"]`. */
    outputGlobs: string[];
}

/** A single generated project (mirrors the core `ProjectModel`). */
export interface ProjectModel {
    index: number;
    name: string;
    /** Workspace-relative POSIX dir, e.g. `packages/p-0007`. */
    dir: string;
    /** Upstream project *names*. */
    dependencies: string[];
    tasks: TaskModel[];
}

/** The complete workspace description (mirrors the core `WorkspaceModel`). */
export interface WorkspaceModel {
    modelVersion: number;
    /** The fully-resolved harness config that produced this model. */
    config: HarnessConfig;
    projects: ProjectModel[];
    /** Deterministic cold-run task counts per task name (`t0`, `t1`, ...). */
    expectedColdExecuted: Record<string, number>;
}

/** Host-specific inputs for rendering the omni layer. */
export interface OmniRenderOptions {
    /** Task `exec` template; `{task_id}` is replaced by the task name. */
    taskCommandTemplate: string;
    /** Per-project cache-key input globs. */
    projectCacheKeyFiles: string[];
}

/**
 * Build the workspace model from a harness config, via the Rust core (wasm).
 * The single source of truth for the graph, task edges, naming, and outputs;
 * task-bench-only config fields are ignored by the core.
 */
export function buildModel(config: HarnessConfig): WorkspaceModel {
    return buildModelWasm(config) as WorkspaceModel;
}

/**
 * Render the omni-layer files (`workspace.omni.yaml` + per-project
 * `project.omni.yaml`) as `[relativePath, contents]` pairs, via the Rust core.
 */
export function renderOmni(
    model: WorkspaceModel,
    options: OmniRenderOptions,
): Array<[string, string]> {
    return renderOmniWasm(model, options) as Array<[string, string]>;
}

/** Schema version of the model payload the wasm core emits. */
export function modelVersion(): number {
    return modelVersionWasm() as number;
}

/** The task names for a config: `t0`, `t1`, ... */
export function taskNames(config: HarnessConfig): string[] {
    return Array.from({ length: config.tasksPerProject }, (_, i) => `t${i}`);
}

/**
 * Number of task executions a correct cold run of `task` performs, from the
 * model's precomputed counts. Returns null when `task` isn't a recognized
 * `tN` within range, so callers can fall back to a looser check.
 */
export function expectedColdExecuted(
    model: WorkspaceModel,
    task: string,
): number | null {
    return model.expectedColdExecuted[task] ?? null;
}
