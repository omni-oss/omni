import { spawn } from "node:child_process";
import fsSync from "node:fs";
import os from "node:os";
import { join } from "node:path";
import { Readable, Writable } from "node:stream";
import { createRpcInstance } from "@omni-oss/bridge-rpc-bootstrap";
import { ResponseStatusCode, StreamTransport } from "@omni-oss/bridge-rpc-core";
import { readBody, readBodyAsJson } from "@omni-oss/bridge-rpc-utils/body";
import type { LogLevel } from "@omni-oss/log";
import { RUNTIME } from "@omni-oss/runtime-utils";
import { afterAll, beforeAll } from "vitest";
import { delay, getHost } from "@/helpers";

// ---------------------------------------------------------------------------
// bridge-service global setup
//
// This setup assumes the service is stateless and can be started once for
// the entire test suite.
// ---------------------------------------------------------------------------

beforeAll(async () => {
    const wsDir = (process.env.WORKSPACE_DIR ?? "")
        .replace(/^\\{2}[?.]\\/, "") // strip \\?\\ or \\.\\  (extended-length / device prefix)
        .replace(/^[A-Za-z]:/, ""); // strip drive letter (e.g. C:)
    if (!wsDir) {
        throw new Error(
            "WORKSPACE_DIR environment variable is not set – " +
                "it must point to the workspace root",
        );
    }

    globalThis.TsRpcProcess = spawn(
        RUNTIME,
        [
            join(wsDir, "services/bridge-service/dist/bridge-service-cli.mjs"),
            "run",
        ],
        {
            stdio: [
                "pipe",
                "pipe",
                process.env.SHOW_LOG_OUTPUT ? "inherit" : "pipe",
            ],
        },
    );

    if (!TsRpcProcess.stdout || !TsRpcProcess.stdin) {
        throw new Error("Failed to spawn RPC process with piped stdio");
    }
    await delay(10);

    if (TsRpcProcess.exitCode !== null) {
        throw new Error(
            `RPC process exited prematurely with code ${TsRpcProcess.exitCode}`,
        );
    }

    const transport = new StreamTransport({
        input: Readable.toWeb(
            TsRpcProcess.stdout,
        ) as ReadableStream<Uint8Array>,
        output: Writable.toWeb(TsRpcProcess.stdin),
    });

    globalThis.TsRpc = createRpcInstance(transport, {
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

                    if (process.env.SHOW_LOG_OUTPUT) {
                        const dt = new Date(json.timestamp);
                        const message = `${json.level.padStart(5, " ").toUpperCase()} [${json.target.join("::")}](${dt.toISOString()}): ${json.message}`;
                        if (
                            json.fields &&
                            Object.keys(json.fields).length > 0
                        ) {
                            console[json.level as LogLevel](
                                message,
                                json.fields,
                            );
                        } else {
                            console[json.level as LogLevel](message);
                        }
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

    globalThis.TsRpcClient = TsRpc.clientHandle;

    await TsRpc.start();
    await delay(10);
});

afterAll(async () => {
    try {
        await delay(10);
        await TsRpc.stop();
    } catch (err) {
        console.error("Error stopping RPC:", err);
    } finally {
        if (TsRpcProcess) {
            TsRpcProcess.kill();
        }
    }
});

// ---------------------------------------------------------------------------
// omni_bridge_test_service global setup
//
// Spawns the Rust `omni_bridge_test_service` binary in `client` mode, which
// registers every service from `bridge_rpc_services` (fs/*, proc/*, /log)
// over its stdin/stdout.  The JS host side acts as the client, making RPC
// calls to exercise those services in end-to-end tests.
// ---------------------------------------------------------------------------

beforeAll(async () => {
    // -----------------------------------------------------------------
    // Resolve the `omni_bridge_test_service` binary path.
    //
    // Follows the same convention as other Rust-binary service tests:
    //   WORKSPACE_DIR  – required; root of the Cargo workspace.
    //   RUST_TARGET    – optional; semicolon-separated list of Cargo
    //                    target triples to look for in target/<T>/release/.
    //                    When absent, target/release/ is used directly.
    // -----------------------------------------------------------------
    const wsDir = process.env.WORKSPACE_DIR ?? "";
    if (!wsDir) {
        throw new Error(
            "WORKSPACE_DIR environment variable is not set – " +
                "it must point to the Cargo workspace root",
        );
    }

    const targetEnv = process.env.RUST_TARGET ?? "";
    const ext = os.platform() === "win32" ? ".exe" : "";
    const binaryName = `omni_bridge_test_service${ext}`;

    const host = await getHost().catch(() => "");
    const compileInfo = `Host: ${host}\nTarget: ${targetEnv}`;

    const defaultPath = join(wsDir, `target/release/${binaryName}`);
    const lookupPaths =
        targetEnv !== ""
            ? [
                  // Include the plain release path when the host triple
                  // is one of the requested targets (i.e. a native build).
                  ...(host !== "" && targetEnv.includes(host)
                      ? [defaultPath]
                      : []),
                  ...targetEnv
                      .split(";")
                      .map((t) =>
                          join(wsDir, `target/${t}/release/${binaryName}`),
                      ),
              ]
            : [defaultPath];

    let testServiceBin = "";
    for (const candidate of lookupPaths) {
        if (fsSync.existsSync(candidate)) {
            testServiceBin = candidate;
            break;
        }
    }

    if (!testServiceBin) {
        throw new Error(
            `Could not find ${binaryName} in:\n${lookupPaths.join(
                "\n",
            )}\n${compileInfo}`,
        );
    }

    globalThis.RsRpcProcess = spawn(
        testServiceBin,
        ["client", "--sys", "in-memory"],
        {
            stdio: [
                "pipe",
                "pipe",
                process.env.SHOW_LOG_OUTPUT ? "inherit" : "pipe",
            ],
        },
    );

    if (!RsRpcProcess.stdout || !RsRpcProcess.stdin) {
        throw new Error(
            "Failed to spawn test service process with piped stdio",
        );
    }

    // Give the binary a moment to initialize its bridge run loop.
    await delay(50);

    if (RsRpcProcess.exitCode !== null) {
        throw new Error(
            `Test service process exited prematurely with code ${RsRpcProcess.exitCode}`,
        );
    }

    const transport = new StreamTransport({
        input: Readable.toWeb(
            RsRpcProcess.stdout,
        ) as ReadableStream<Uint8Array>,
        output: Writable.toWeb(RsRpcProcess.stdin),
    });

    // The host (JS) side does not need to expose any services back to the
    // Rust binary in client mode – no tracing subscriber is installed in
    // the child, so no /log frames are ever sent upstream.
    globalThis.RsRpc = createRpcInstance(transport);
    globalThis.RsRpcClient = RsRpc.clientHandle;

    await RsRpc.start();
    await delay(10);
});

afterAll(async () => {
    try {
        await delay(10);
        await RsRpc.stop();
    } catch (err) {
        console.error("Error stopping test service RPC:", err);
    } finally {
        if (RsRpcProcess) {
            RsRpcProcess.kill();
        }
    }
});
