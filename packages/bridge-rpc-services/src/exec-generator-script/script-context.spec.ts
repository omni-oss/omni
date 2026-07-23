import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { Log, type Logger, type LoggerFactory } from "@omni-oss/log";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

// ──────────────────────────────────────────────────────────────────────────
// Mocks
// ──────────────────────────────────────────────────────────────────────────
// `BridgeRpcSystem.create` performs a real RPC handshake to build a `System`.
// We stub it so the context-assembly logic can be unit tested without a live
// bridge connection, and so we can assert the client handle is forwarded.
const { systemCreateMock, SYSTEM_MARKER } = vi.hoisted(() => ({
    systemCreateMock: vi.fn(),
    SYSTEM_MARKER: {
        fs: { __marker: "fs" },
        proc: {
            currentDir: () => "/host/cwd",
            setCurrentDir: async () => {},
            args: () => [],
            env: () => ({}),
        },
    },
}));

vi.mock("@omni-oss/bridge-rpc-system-interface", () => ({
    BridgeRpcSystem: {
        create: systemCreateMock,
    },
}));

import { DefaultScriptContext } from "./script-context";

const TMP_DIR = path.join(
    fs.realpathSync(os.tmpdir()),
    `test-${crypto.randomUUID()}`,
);
const OUTPUT_DIR = path.join(TMP_DIR, "output");

// ──────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────
const PROVIDED_LOGGER = { id: "provided" } as unknown as Logger;
const AMBIENT_LOGGER = { id: "ambient" } as unknown as Logger;
const AMBIENT_FACTORY: LoggerFactory = { get: () => AMBIENT_LOGGER };

function withLog<T>(fn: () => Promise<T>): Promise<T> {
    return Log.withRoot(AMBIENT_FACTORY, ["test"], fn);
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────
describe("DefaultScriptContext", () => {
    beforeEach(() => {
        systemCreateMock.mockReset();
        systemCreateMock.mockResolvedValue(SYSTEM_MARKER);
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    test("builds the system from the provided client handle", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: false,
            data: null,
            logger: PROVIDED_LOGGER,
            outputDir: OUTPUT_DIR,
        });

        expect(systemCreateMock).toHaveBeenCalledTimes(1);
        // Called with the client handle plus the env-rule options derived from
        // the active policy (no policy installed in this test → `undefined`,
        // i.e. the snapshot passes through unfiltered).
        expect(systemCreateMock).toHaveBeenCalledWith(ClientHandle.DUMMY, {
            envRules: undefined,
        });
        // The base system is wrapped to virtualise the current directory, so
        // `sys` is not the raw marker but its `proc.currentDir()` starts at the
        // provided `outputDir`.
        expect(ctx.sys).not.toBe(SYSTEM_MARKER);
        expect(ctx.sys.proc.currentDir()).toBe(OUTPUT_DIR);
    });

    test("uses the explicitly provided logger", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: false,
            data: null,
            logger: PROVIDED_LOGGER,
            outputDir: OUTPUT_DIR,
        });

        expect(ctx.log).toBe(PROVIDED_LOGGER);
    });

    test("falls back to the ambient Log instance when no logger is given", async () => {
        const ctx = await withLog(() =>
            DefaultScriptContext.create({
                clientHandle: ClientHandle.DUMMY,
                dryRun: false,
                data: null,
                outputDir: OUTPUT_DIR,
            }),
        );

        expect(ctx.log).toBe(AMBIENT_LOGGER);
    });

    test("reflects dryRun=true on isDryRun", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: true,
            data: null,
            logger: PROVIDED_LOGGER,
            outputDir: OUTPUT_DIR,
        });

        expect(ctx.isDryRun).toBe(true);
    });

    test("reflects dryRun=false on isDryRun", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: false,
            data: null,
            logger: PROVIDED_LOGGER,
            outputDir: OUTPUT_DIR,
        });

        expect(ctx.isDryRun).toBe(false);
    });

    test("exposes the provided data verbatim", async () => {
        const data = { hello: "world", nested: [1, 2, 3] };
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: false,
            data,
            logger: PROVIDED_LOGGER,
            outputDir: OUTPUT_DIR,
        });

        expect(ctx.data).toBe(data);
    });
});
