import { nativeTarget } from "./native";
import type { Target } from "./types";
import { workerTarget } from "./worker";

const targets: Record<string, Target> = {
    native: nativeTarget,
    worker: workerTarget,
};

/** Targets to run, from `REMOTE_CACHE_TEST_TARGETS` (comma separated; all by default). */
export function resolveTargets(): Target[] {
    const names =
        process.env.REMOTE_CACHE_TEST_TARGETS?.split(",")
            .map((n) => n.trim())
            .filter(Boolean) ?? Object.keys(targets);
    return names.map((name) => {
        const target = targets[name];
        if (!target) {
            throw new Error(
                `Unknown target "${name}"; expected one of: ${Object.keys(targets).join(", ")}`,
            );
        }
        return target;
    });
}

export type { RunningTarget, Target } from "./types";
