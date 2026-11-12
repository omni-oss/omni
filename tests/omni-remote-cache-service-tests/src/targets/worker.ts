import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { sleep } from "../utils";
import type { Target } from "./types";
import { waitForServer } from "./wait";

export interface WorkerVariant {
    name: string;
    basePort: number;
    /**
     * Prepares the worker dir and persistence dir, and returns the extra
     * wrangler args (config and vars) plus an optional SQL runner.
     */
    prepare: (ctx: { workerDir: string; state: string }) => Promise<{
        args: string[];
        sql?: ((statement: string) => Promise<string>) | undefined;
        cleanup?: () => void;
    }>;
}

/**
 * The Cloudflare worker under `wrangler dev --local` (Miniflare), with a
 * throwaway persistence dir so every run starts from empty storage.
 * Requires the worker to be buildable (`worker-build`) and `wrangler` or `bunx`.
 */
export function workerTargetFor(variant: WorkerVariant): Target {
    return {
        name: variant.name,
        basePort: variant.basePort,
        async start(port) {
            const workspace = process.env.WORKSPACE_DIR;
            if (!workspace) {
                throw new Error(
                    "WORKSPACE_DIR is required for the worker target",
                );
            }
            const workerDir = path.join(
                workspace,
                "services",
                "omni_remote_cache_worker",
            );
            const state = fs.mkdtempSync(
                path.join(os.tmpdir(), "orcs-worker-"),
            );

            const args = [
                "dev",
                "--local",
                "--port",
                String(port),
                "--persist-to",
                state,
            ];
            const prepared = await variant.prepare({ workerDir, state });
            args.push(...prepared.args);
            const child = spawn("bunx", ["wrangler@latest", ...args], {
                cwd: workerDir,
                env: process.env,
                stdio: "pipe",
                detached: true,
            });

            const output: string[] = [];
            child.stdout?.on("data", (d) => output.push(d.toString()));
            child.stderr?.on("data", (d) => output.push(d.toString()));

            const kill = (sig: NodeJS.Signals) => {
                try {
                    if (child.pid) process.kill(-child.pid, sig);
                } catch {
                    // already gone
                }
            };
            const cleanup = () => {
                prepared.cleanup?.();
                fs.rmSync(state, { recursive: true, force: true });
            };

            // Vitest never tells a shared target to stop, so reap on exit.
            process.once("exit", () => {
                kill("SIGTERM");
                cleanup();
            });

            const apiBaseUrl = `http://localhost:${port}/api`;
            try {
                // includes a cargo build via wrangler's custom build step
                await waitForServer(apiBaseUrl, {
                    tries: 600,
                    delayMs: 500,
                    fetchTimeoutMs: 1000,
                    exited: () =>
                        child.exitCode != null
                            ? `exited with code ${child.exitCode}`
                            : null,
                    describe: () => output.join(""),
                });
            } catch (e) {
                kill("SIGKILL");
                cleanup();
                throw e;
            }

            return {
                apiBaseUrl,
                sql: prepared.sql,
                async stop() {
                    kill("SIGTERM");
                    await sleep(1500);
                    kill("SIGKILL");
                    cleanup();
                },
            };
        },
    };
}

/** The worker with ownership data and API keys from `ORCS_CONFIG`. */
export const workerTarget: Target = workerTargetFor({
    name: "worker",
    basePort: 3500,
    async prepare() {
        const config = fs
            .readFileSync(path.join(process.cwd(), "orcs.config.json"), "utf8")
            .replace(/\n/g, "");
        return { args: ["--var", `ORCS_CONFIG:${config}`] };
    },
});
