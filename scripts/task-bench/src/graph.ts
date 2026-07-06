import type { HarnessConfig } from "./config";

/** A single generated project and the upstream projects it depends on. */
export interface ProjectNode {
    /** Zero-based index in the generated set. */
    index: number;
    /** Package name, e.g. `bench-p0007`. */
    name: string;
    /** Workspace-relative POSIX directory, e.g. `packages/bench-p0007`. */
    dir: string;
    /** Indices of upstream projects this project depends on. */
    dependencies: number[];
}

/**
 * Deterministic PRNG (mulberry32) so a given seed always yields the same graph.
 */
function makeRng(seed: number): () => number {
    let a = seed >>> 0;
    return () => {
        a = (a + 0x6d2b79f5) | 0;
        let t = Math.imul(a ^ (a >>> 15), 1 | a);
        t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

function padWidth(count: number): number {
    return Math.max(4, String(count - 1).length);
}

export function projectName(config: HarnessConfig, index: number): string {
    const width = padWidth(config.projects);
    return `${config.projectPrefix}${String(index).padStart(width, "0")}`;
}

/** Evenly sample up to `count` indices from the inclusive range [start, end]. */
function evenlySample(start: number, end: number, count: number): number[] {
    const size = end - start + 1;
    if (size <= 0 || count <= 0) return [];
    if (count >= size) {
        return Array.from({ length: size }, (_, i) => start + i);
    }
    const picked: number[] = [];
    for (let i = 0; i < count; i++) {
        // Spread picks across the range deterministically.
        const offset = Math.round((i * (size - 1)) / (count - 1 || 1));
        picked.push(start + offset);
    }
    return [...new Set(picked)];
}

function computeDependencies(
    config: HarnessConfig,
    index: number,
    rng: () => number,
): number[] {
    const { strategy, layers, fanout, edgeProbability } = config.dependency;

    switch (strategy) {
        case "isolated":
            return [];

        case "chain":
            return index > 0 ? [index - 1] : [];

        case "fan-out":
            // Every project (except the root) depends on the single root project.
            return index > 0 ? [0] : [];

        case "layered": {
            const perLayer = Math.ceil(config.projects / layers);
            const layer = Math.floor(index / perLayer);
            if (layer === 0) return [];
            const prevStart = (layer - 1) * perLayer;
            const prevEnd = Math.min(layer * perLayer, config.projects) - 1;
            return evenlySample(prevStart, prevEnd, fanout);
        }

        case "random": {
            if (index === 0) return [];
            const deps: number[] = [];
            for (let j = 0; j < index; j++) {
                if (rng() < edgeProbability) deps.push(j);
            }
            if (fanout > 0 && deps.length > fanout) {
                // Keep the `fanout` closest ancestors for a shallower graph.
                return deps.slice(deps.length - fanout);
            }
            return deps;
        }

        default:
            return [];
    }
}

/**
 * Build the full project graph for a config. Dependencies always point to
 * lower indices, guaranteeing an acyclic graph.
 */
export function buildGraph(config: HarnessConfig): ProjectNode[] {
    const rng = makeRng(config.seed);
    const nodes: ProjectNode[] = [];
    for (let index = 0; index < config.projects; index++) {
        const name = projectName(config, index);
        nodes.push({
            index,
            name,
            dir: `packages/${name}`,
            dependencies: computeDependencies(config, index, rng),
        });
    }
    return nodes;
}

/** The task names generated for every project: `t0`, `t1`, ... */
export function taskNames(config: HarnessConfig): string[] {
    return Array.from({ length: config.tasksPerProject }, (_, i) => `t${i}`);
}

/**
 * Number of task executions a *correct* cold run performs when invoking `task`
 * across every project. `task` is scheduled for all projects, so intra-project
 * chaining pulls in each project's whole `t0..tK` prefix, while upstream
 * (`^tK`) edges add no new nodes (every project already runs `tK`). Hence:
 *
 *   projects × (chainWithinProject ? K + 1 : 1)
 *
 * Returns null when `task` isn't a recognized `tN` within range, so callers can
 * fall back to a looser check.
 */
export function expectedColdExecuted(
    config: HarnessConfig,
    task: string,
): number | null {
    const match = /^t(\d+)$/.exec(task);
    if (!match) return null;
    const k = Number(match[1]);
    if (!Number.isInteger(k) || k < 0 || k >= config.tasksPerProject) {
        return null;
    }
    const perProject = config.task.chainWithinProject ? k + 1 : 1;
    return config.projects * perProject;
}
