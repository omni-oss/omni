import { afterAll, beforeAll, describe } from "vitest";

import { resolveTargets, type Target } from "./targets";

export interface TargetContext {
    readonly name: string;
    readonly apiBaseUrl: string;
}

let nextPort = 3400;

/**
 * Data-driven suite: registers `define` once per target (native binary,
 * Cloudflare worker, ...) so a single run exercises every implementation.
 * Restrict with `REMOTE_CACHE_TEST_TARGETS=native,worker`.
 */
export function describeTargets(
    name: string,
    define: (target: TargetContext) => void,
) {
    describe.each(resolveTargets())("$name: " + name, (target: Target) => {
        let running: Awaited<ReturnType<Target["start"]>> | null = null;

        // Booting the worker includes a cargo build, far beyond the default timeout.
        beforeAll(async () => {
            running = await target.start(nextPort++);
        }, 600_000);

        afterAll(async () => {
            await running?.stop();
            running = null;
        });

        define({
            name: target.name,
            get apiBaseUrl() {
                if (!running) throw new Error("target is not running");
                return running.apiBaseUrl;
            },
        });
    });
}
