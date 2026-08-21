import { type ChildProcess, spawn } from "node:child_process";
import { test as baseTest } from "vitest";
import { resolveServiceBinOrThrow } from "./binary";
import { getHost, sleep, withTimeout } from "./utils";

const ports = new Set<number>();

const timeoutFetch = withTimeout(fetch, 100);

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
            const host = await getHost().catch(() => "");
            const { path: omniPath, compileInfo } = resolveServiceBinOrThrow({
                binName: "omni_remote_cache_service",
                host,
            });

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

            const output = [] as string[];

            childProcess.stdout?.on("data", (data) => {
                output.push(data.toString());
            });
            childProcess.stderr?.on("data", (data) => {
                output.push(data.toString());
            });

            // we're not trying to get a successful response, just to make sure the server is up and can respond
            let currentTry = 0;
            let didConnect = false;
            let error: Error | null = null;
            const MAX_TRIES = 10;
            while (currentTry < MAX_TRIES) {
                try {
                    await timeoutFetch(apiBaseUrl);
                    didConnect = true;
                    break;
                } catch (e) {
                    if (error instanceof Error) {
                        error = e as Error;
                    }
                }

                if (childProcess.exitCode != null) {
                    throw new Error(
                        `Child process exited with code ${childProcess.exitCode}:\n${output.join("\n")}\n${compileInfo}`,
                    );
                }

                currentTry++;
                // add a small delay to ensure the server is ready
                await sleep(100);
            }

            if (!didConnect) {
                if (error) {
                    throw error;
                }
                throw new Error(
                    `Failed to connect to server: ${apiBaseUrl}\n${output.join(
                        "\n",
                    )}\n${compileInfo}`,
                );
            }

            try {
                await use(childProcess);
            } finally {
                const result = childProcess.kill("SIGTERM");
                await sleep(100);

                if (
                    !result &&
                    childProcess.exitCode !== null &&
                    !childProcess.killed
                ) {
                    childProcess.kill("SIGKILL");
                }
            }
        },
        { scope: "test", auto: true },
    ],
});
