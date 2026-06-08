import { spawn } from "node:child_process";
import { join } from "node:path";
import { Readable, Writable } from "node:stream";
import { createRpcInstance } from "@omni-oss/bridge-rpc-bootstrap";
import { ResponseStatusCode, StreamTransport } from "@omni-oss/bridge-rpc-core";
import { readBody, readBodyAsJson } from "@omni-oss/bridge-rpc-utils/body";
import type { LogLevel } from "@omni-oss/log";
import { RUNTIME } from "@omni-oss/runtime-utils";
import { afterAll, beforeAll } from "vitest";
import { delay } from "@/helpers";

/**
 * This setup assumes the service is stateless and can be started once for the entire test suite.
 */

beforeAll(async () => {
    globalThis.RpcProcess = spawn(
        RUNTIME,
        [
            join(
                __dirname,
                "../../../../services/bridge-service/dist/bridge-service-cli.mjs",
            ),
            "run",
        ],
        {
            stdio: "pipe",
        },
    );

    if (!RpcProcess.stdout || !RpcProcess.stdin) {
        throw new Error("Failed to spawn RPC process with piped stdio");
    }
    await delay(10);

    if (RpcProcess.exitCode !== null) {
        throw new Error(
            `RPC process exited prematurely with code ${RpcProcess.exitCode}`,
        );
    }

    RpcProcess.on("error", (err) => {
        console.error("RPC process error:", err);
    });

    RpcProcess.on("exit", (code, signal) => {
        console.log(
            `RPC process exited with code ${code} and signal ${signal}`,
        );
    });

    RpcProcess.stderr?.on("data", (data) => {
        console.error(`RPC process stderr: ${data}`);
    });

    RpcProcess.stderr?.on("error", (err) => {
        console.error("Error reading RPC process stderr:", err);
    });

    RpcProcess.stdout?.on("error", (err) => {
        console.error("Error reading RPC process stdout:", err);
    });

    RpcProcess.stdin?.on("error", (err) => {
        console.error("Error writing to RPC process stdin:", err);
    });

    const transport = new StreamTransport({
        input: Readable.toWeb(RpcProcess.stdout) as ReadableStream<Uint8Array>,
        output: Writable.toWeb(RpcProcess.stdin),
    });

    globalThis.Rpc = createRpcInstance(transport, {
        services: [
            {
                path: "/log",
                handler: async (ctx) => {
                    const json = await readBodyAsJson<{
                        timestamp: number;
                        level: string;
                        target: string[];
                        message: string;
                        fields?: Record<string, unknown>;
                    }>(ctx.request);

                    const dt = new Date(json.timestamp);
                    const message = `${json.level.padStart(5, " ").toUpperCase()} [${json.target.join("::")}](${dt.toISOString()}): ${json.message}`;
                    if (json.fields && Object.keys(json.fields).length > 0) {
                        console[json.level as LogLevel](message, json.fields);
                    } else {
                        console[json.level as LogLevel](message);
                    }

                    await ctx.response
                        .start(ResponseStatusCode.SUCCESS)
                        .then((res) => res.end());
                },
            },
            {
                path: "/proc/snapshot",
                handler: async (ctx) => {
                    await readBody(ctx.request);

                    await ctx.response
                        .start(ResponseStatusCode.SUCCESS, {
                            returns: {
                                current_dir: "/home/user/test",
                                args: [],
                                env: {},
                            },
                        })
                        .then((res) => res.end());
                },
            },
        ],
    });

    globalThis.RpcClient = Rpc.clientHandle;

    await Rpc.start();
    await delay(10);
});

afterAll(async () => {
    try {
        await delay(10);
        await Rpc.stop();
    } catch (err) {
        console.error("Error stopping RPC:", err);
    } finally {
        if (RpcProcess) {
            RpcProcess.kill();
        }
    }
});
