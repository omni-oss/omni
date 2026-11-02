import { nativeTarget } from "./native";
import type { Target } from "./types";
import { workerTarget } from "./worker";

const targets: Record<string, Target> = {
    native: nativeTarget,
    worker: workerTarget,
};

/** Selected with `REMOTE_CACHE_TEST_TARGET` (`native` by default). */
export function resolveTarget(): Target {
    const name = process.env.REMOTE_CACHE_TEST_TARGET ?? "native";
    const target = targets[name];
    if (!target) {
        throw new Error(
            `Unknown REMOTE_CACHE_TEST_TARGET "${name}"; expected one of: ${Object.keys(targets).join(", ")}`,
        );
    }
    return target;
}

export type { RunningTarget, Target } from "./types";
