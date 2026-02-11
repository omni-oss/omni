import { type ChildProcess, spawn } from "node:child_process";
import fsSync from "node:fs";
import { test as baseTest } from "vitest";

const ports = new Set<number>();

export const test = baseTest.extend<{
    port: number;
    apiBaseUrl: string;
    childProcess: ChildProcess;
}>({
    port: [
        // biome-ignore lint/correctness/noEmptyPattern: expected to have empty pattern
        async ({}, use) => {
            const maxPort = ports
                .entries()
                .reduce((acc, cur) => Math.max(acc, cur[1]), 3399);
            const minPort = ports
                .entries()
                .reduce((acc, cur) => Math.min(acc, cur[1]), 3400);

            let port: number | null = null;
            for (let i = minPort; i <= maxPort; i++) {
                if (!ports.has(i)) {
                    port = i;
                    break;
                }
            }

            const newPort = port ?? maxPort + 1;

            ports.add(newPort);
            await use(newPort);
            ports.delete(newPort);
        },
        { scope: "test" },
    ],
    apiBaseUrl: [
        async ({ port }, use) => {
            const apiBaseUrl = `http://localhost:${port}/api`;
            await use(apiBaseUrl);
        },
        { scope: "test" },
    ],
    childProcess: [
        async ({ apiBaseUrl, port }, use) => {
            const wsDir = process.env.WORKSPACE_DIR ?? "";
            if (!wsDir) {
                throw new Error("WORKSPACE_DIR is not set");
            }

            const target = process.env.RUST_TARGET ?? "";

            const ext =
                target && target !== "" && target.includes("windows")
                    ? ".exe"
                    : "";

            let omniPath = "";

            const defaultPath = `${wsDir}/target/release/omni_remote_cache_service${ext}`;
            const lookupPaths =
                target !== ""
                    ? [
                          `${wsDir}/target/${target}/release/omni_remote_cache_service${ext}`,
                          defaultPath,
                      ]
                    : [defaultPath];

            for (const lookupPath of lookupPaths) {
                if (fsSync.existsSync(lookupPath)) {
                    omniPath = lookupPath;
                    break;
                }
            }

            const childProcess = spawn(
                omniPath,
                [
                    "serve",
                    "--listen",
                    `0.0.0.0:${port}`,
                    "-b",
                    "in-memory",
                    "--routes.api-prefix",
                    "/api",
                    "--config",
                    "orcs.config.json",
                    "--config-type",
                    "file",
                ],
                {
                    env: process.env,
                    stdio: "pipe",
                    cwd: process.cwd(),
                },
            );

            // we're not trying to get a successful response, just to make sure the server is up and can respond
            let currentTry = 0;
            let didConnect = false;
            let error: Error | null = null;
            const MAX_TRIES = 10;
            while (currentTry < MAX_TRIES) {
                try {
                    await fetch(apiBaseUrl);
                    didConnect = true;
                    break;
                } catch (e) {
                    if (error instanceof Error) {
                        error = e as Error;
                    }
                }

                currentTry++;
                // add a small delay to ensure the server is ready
                await new Promise((resolve) => setTimeout(resolve, 100));
            }

            if (!didConnect) {
                if (error) {
                    console.error(error);
                }
                throw new Error(`Failed to connect to server: ${apiBaseUrl}`);
            }

            await use(childProcess);
            if (!childProcess.kill("SIGTERM")) {
                throw new Error(
                    `Failed to kill child process: ${childProcess.pid}`,
                );
            }
        },
        { scope: "test", auto: true },
    ],
});
