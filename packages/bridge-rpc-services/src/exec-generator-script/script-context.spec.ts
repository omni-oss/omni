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
    SYSTEM_MARKER: { __marker: "system" },
}));

vi.mock("@omni-oss/bridge-rpc-system-interface", () => ({
    BridgeRpcSystem: {
        create: systemCreateMock,
    },
}));

import { DefaultScriptContext } from "./script-context";

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
            logger: PROVIDED_LOGGER,
        });

        expect(systemCreateMock).toHaveBeenCalledTimes(1);
        expect(systemCreateMock).toHaveBeenCalledWith(ClientHandle.DUMMY);
        expect(ctx.sys).toBe(SYSTEM_MARKER);
    });

    test("uses the explicitly provided logger", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: false,
            logger: PROVIDED_LOGGER,
        });

        expect(ctx.log).toBe(PROVIDED_LOGGER);
    });

    test("falls back to the ambient Log instance when no logger is given", async () => {
        const ctx = await withLog(() =>
            DefaultScriptContext.create({
                clientHandle: ClientHandle.DUMMY,
                dryRun: false,
            }),
        );

        expect(ctx.log).toBe(AMBIENT_LOGGER);
    });

    test("reflects dryRun=true on isDryRun", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: true,
            logger: PROVIDED_LOGGER,
        });

        expect(ctx.isDryRun).toBe(true);
    });

    test("reflects dryRun=false on isDryRun", async () => {
        const ctx = await DefaultScriptContext.create({
            clientHandle: ClientHandle.DUMMY,
            dryRun: false,
            logger: PROVIDED_LOGGER,
        });

        expect(ctx.isDryRun).toBe(false);
    });
});
