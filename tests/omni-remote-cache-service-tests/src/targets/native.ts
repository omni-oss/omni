import { spawn } from "node:child_process";

import { resolveServiceBinOrThrow } from "../binary";
import { getHost, sleep } from "../utils";
import { waitForServer } from "./wait";
import type { Target } from "./types";

export const nativeTarget: Target = {
    name: "native",
    basePort: 3400,
    async start(port) {
        const host = await getHost().catch(() => "");
        const { path: bin, compileInfo } = resolveServiceBinOrThrow({
            binName: "omni_remote_cache_service",
            host,
        });

        const child = spawn(
            bin,
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
            { env: process.env, stdio: "pipe", cwd: process.cwd() },
        );

        const output: string[] = [];
        child.stdout?.on("data", (d) => output.push(d.toString()));
        child.stderr?.on("data", (d) => output.push(d.toString()));

        const apiBaseUrl = `http://localhost:${port}/api`;
        const describe = () => `${output.join("\n")}\n${compileInfo}`;

        try {
            await waitForServer(apiBaseUrl, {
                tries: 10,
                delayMs: 100,
                fetchTimeoutMs: 100,
                exited: () =>
                    child.exitCode != null
                        ? `exited with code ${child.exitCode}`
                        : null,
                describe,
            });
        } catch (e) {
            child.kill("SIGKILL");
            throw e;
        }

        return {
            apiBaseUrl,
            async stop() {
                if (!child.kill("SIGTERM")) {
                    return;
                }
                await sleep(100);
                if (child.exitCode === null) {
                    child.kill("SIGKILL");
                }
            },
        };
    },
};
