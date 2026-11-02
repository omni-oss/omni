import { afterAll, beforeAll, test as baseTest } from "vitest";

import { type RunningTarget, resolveTarget } from "./targets";

const ports = new Set<number>();
const target = resolveTarget();

function allocatePort(): number {
    let port = 3400;
    while (ports.has(port)) port++;
    ports.add(port);
    return port;
}

let shared: Promise<RunningTarget> | null = null;
function startShared() {
    if (!shared) {
        const port = allocatePort();
        shared = target.start(port);
    }
    return shared;
}

// Booting the worker includes a cargo build, far beyond the per-test timeout.
beforeAll(async () => {
    if (target.shared) await startShared();
}, 600_000);

afterAll(async () => {
    if (shared) await (await shared).stop();
    shared = null;
});

export const test = baseTest.extend<{
    targetName: string;
    apiBaseUrl: string;
}>({
    // oxlint-disable-next-line no-empty-pattern
    targetName: async ({}, use) => {
        await use(target.name);
    },
    apiBaseUrl: [
        // oxlint-disable-next-line no-empty-pattern
        async ({}, use) => {
            if (target.shared) {
                await use((await startShared()).apiBaseUrl);
                return;
            }
            const port = allocatePort();
            const running = await target.start(port);
            try {
                await use(running.apiBaseUrl);
            } finally {
                await running.stop();
                ports.delete(port);
            }
        },
        { scope: "test", auto: true },
    ],
});
