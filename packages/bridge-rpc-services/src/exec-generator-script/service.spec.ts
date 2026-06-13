import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
    ClientHandle,
    Id,
    ResponseStatusCode,
    ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import {
    type Frame,
    FrameType,
    type RequestError,
} from "@omni-oss/bridge-rpc-core/frame";
import {
    PendingResponse,
    Request,
    RequestFrameEvent,
} from "@omni-oss/bridge-rpc-core/server";
import { Mpsc, type MpscReceiver, Oneshot } from "@omni-oss/channels";
import { Log, type Logger, type LoggerFactory } from "@omni-oss/log";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

// ──────────────────────────────────────────────────────────────────────────
// Mocks
// ──────────────────────────────────────────────────────────────────────────
// `DefaultScriptContext.create` reaches out over RPC (BridgeRpcSystem) to
// build a real `System`, which a unit test can't satisfy. We stub it so the
// service-level behaviour (payload parsing, default-export validation,
// per-script invocation, error wrapping) can be exercised in isolation while
// still proving the context is threaded into each script.
const { createContextMock } = vi.hoisted(() => ({
    createContextMock: vi.fn(),
}));

vi.mock("./script-context", () => ({
    DefaultScriptContext: {
        create: createContextMock,
    },
}));

import { ExecGeneratorScript } from "./service";

// ──────────────────────────────────────────────────────────────────────────
// Fixtures
// ──────────────────────────────────────────────────────────────────────────
const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), "__fixtures__");

const genAPath = join(FIXTURES, "gen-a.mjs");
const genBPath = join(FIXTURES, "gen-b.mjs");
const genAsyncPath = join(FIXTURES, "gen-async.mjs");
const genThrowsPath = join(FIXTURES, "gen-throws.mjs");
const genRejectsPath = join(FIXTURES, "gen-rejects.mjs");
const noDefaultPath = join(FIXTURES, "no-default.mjs");
const nonFnDefaultPath = join(FIXTURES, "non-fn-default.mjs");

// A path that is guaranteed not to resolve.
const missingPath = pathToFileURL(join(FIXTURES, "does-not-exist.mjs")).href;

// ──────────────────────────────────────────────────────────────────────────
// Generator-call registry (populated by the fixtures via globalThis)
// ──────────────────────────────────────────────────────────────────────────
type GenCall = {
    name: string;
    isDryRun: boolean;
    hasSys: boolean;
    hasLog: boolean;
};

const GEN_CALLS_KEY = "__OMNI_GEN_CALLS__";

function getCalls(): GenCall[] {
    return (
        ((globalThis as Record<string, unknown>)[GEN_CALLS_KEY] as
            | GenCall[]
            | undefined) ?? []
    );
}

function clearCalls(): void {
    delete (globalThis as Record<string, unknown>)[GEN_CALLS_KEY];
}

// ──────────────────────────────────────────────────────────────────────────
// Log scoping helpers
// ──────────────────────────────────────────────────────────────────────────
// The runner requires an initialised ambient logger. The logger itself is
// never exercised by the fixtures (it only gets handed to the stubbed
// context), so a marker object cast to `Logger` is sufficient.
const TEST_LOGGER = { id: "test-logger" } as unknown as Logger;
const TEST_FACTORY: LoggerFactory = { get: () => TEST_LOGGER };

function withLog<T>(fn: () => Promise<T>): Promise<T> {
    return Log.withRoot(TEST_FACTORY, ["test"], fn);
}

const SYS_MARKER = { __marker: "sys" };

// ──────────────────────────────────────────────────────────────────────────
// Test harness (mirrors exec-script/service.spec.ts)
// ──────────────────────────────────────────────────────────────────────────
const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

type Harness = {
    context: ServiceContext;
    responseFrames: MpscReceiver<Frame>;
};

function makeHarness(bodyBytes: Uint8Array): Harness {
    const id = Id.create();
    const requestError = new Oneshot<RequestError>();
    const responseChannel = new Mpsc<Frame>();

    const events = (async function* () {
        yield RequestFrameEvent.bodyChunk(bodyBytes);
        yield RequestFrameEvent.end();
    })();

    const request = new Request(
        id,
        "exec-generator-script",
        {},
        events,
        requestError.receiver,
    );
    const response = new PendingResponse(id, responseChannel.sender);

    return {
        context: ServiceContext.fromRequestAndResponse(request, response),
        responseFrames: responseChannel.receiver,
    };
}

function makeJsonHarness(body: unknown): Harness {
    return makeHarness(TEXT_ENCODER.encode(JSON.stringify(body)));
}

function makePayload(
    paths: string[],
    dryRun: boolean,
): { paths: string[]; params: { dry_run: boolean } } {
    return { paths, params: { dry_run: dryRun } };
}

/** Drains response frames until RESPONSE_END (or RESPONSE_ERROR) is observed. */
async function drainResponse(receiver: MpscReceiver<Frame>): Promise<Frame[]> {
    const frames: Frame[] = [];
    while (true) {
        const result = await receiver.next();
        if (result.done) break;
        frames.push(result.value);
        if (
            result.value.type === FrameType.RESPONSE_END ||
            result.value.type === FrameType.RESPONSE_ERROR
        ) {
            break;
        }
    }
    return frames;
}

type ResponseSummary = {
    status: number;
    body: string;
    frames: Frame[];
};

async function runService(
    harness: Harness,
    service: ExecGeneratorScript,
): Promise<ResponseSummary> {
    const [, frames] = await Promise.all([
        service.run(harness.context),
        drainResponse(harness.responseFrames),
    ]);

    const startFrame = frames.find((f) => f.type === FrameType.RESPONSE_START);
    if (!startFrame || startFrame.type !== FrameType.RESPONSE_START) {
        throw new Error("no RESPONSE_START frame observed");
    }

    const chunks: Uint8Array[] = [];
    for (const frame of frames) {
        if (frame.type === FrameType.RESPONSE_BODY_CHUNK) {
            chunks.push(frame.data.chunk);
        }
    }
    const total = chunks.reduce((s, c) => s + c.byteLength, 0);
    const merged = new Uint8Array(total);
    let offset = 0;
    for (const c of chunks) {
        merged.set(c, offset);
        offset += c.byteLength;
    }

    return {
        status: Number(startFrame.data.status),
        body: TEXT_DECODER.decode(merged),
        frames,
    };
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────
describe("ExecGeneratorScript", () => {
    beforeEach(() => {
        clearCalls();
        createContextMock.mockReset();
        createContextMock.mockImplementation(
            async (opts: {
                clientHandle: ClientHandle;
                dryRun: boolean;
                logger?: Logger;
            }) => ({
                sys: SYS_MARKER,
                log: opts.logger,
                isDryRun: opts.dryRun,
            }),
        );
    });

    afterEach(() => {
        clearCalls();
    });

    describe("successful execution", () => {
        test("runs a single generator and returns SUCCESS", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([genAPath], false));

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            expect(result.body).toBe("");
            expect(result.frames.map((f) => f.type)).toEqual([
                FrameType.RESPONSE_START,
                FrameType.RESPONSE_END,
            ]);
            expect(getCalls()).toEqual([
                { name: "a", isDryRun: false, hasSys: true, hasLog: true },
            ]);
        });

        test("runs multiple generators in the order their paths were given", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([genBPath, genAPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            expect(getCalls().map((c) => c.name)).toEqual(["b", "a"]);
        });

        test("awaits async generators before responding", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([genAsyncPath], false));

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            // If the async fn weren't awaited, the call wouldn't be recorded
            // by the time the response resolved.
            expect(getCalls().map((c) => c.name)).toEqual(["async"]);
        });

        test("creates the context once, with the request client and ambient logger", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([genAPath, genBPath], false),
            );

            await withLog(() => runService(harness, service));

            expect(createContextMock).toHaveBeenCalledTimes(1);
            expect(createContextMock).toHaveBeenCalledWith({
                clientHandle: ClientHandle.DUMMY,
                dryRun: false,
                logger: TEST_LOGGER,
            });
        });

        test("treats an empty paths array as a no-op success", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([], false));

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            // The context is still created, but no generator runs.
            expect(createContextMock).toHaveBeenCalledTimes(1);
            expect(getCalls()).toEqual([]);
        });
    });

    describe("dry_run param", () => {
        test("threads dry_run=true into the context handed to generators", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([genAPath], true));

            await withLog(() => runService(harness, service));

            expect(createContextMock).toHaveBeenCalledWith(
                expect.objectContaining({ dryRun: true }),
            );
            expect(getCalls()).toEqual([
                { name: "a", isDryRun: true, hasSys: true, hasLog: true },
            ]);
        });

        test("threads dry_run=false into the context handed to generators", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([genAPath], false));

            await withLog(() => runService(harness, service));

            expect(getCalls()).toEqual([
                { name: "a", isDryRun: false, hasSys: true, hasLog: true },
            ]);
        });
    });

    describe("default-export validation", () => {
        test("fails with 500 when a script has no default export", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([noDefaultPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toMatch(/do not export a default function/i);
            expect(result.body).toContain(noDefaultPath);
            // Validation happens before any context is created.
            expect(createContextMock).not.toHaveBeenCalled();
        });

        test("fails with 500 when the default export is not a function", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([nonFnDefaultPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toMatch(/do not export a default function/i);
            expect(result.body).toContain(nonFnDefaultPath);
        });

        test("lists every offending script when several lack a default function", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([noDefaultPath, nonFnDefaultPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toContain(noDefaultPath);
            expect(result.body).toContain(nonFnDefaultPath);
        });
    });

    describe("Log initialization guard", () => {
        test("fails with 500 when run outside an initialized Log scope", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([genAPath], false));

            // Note: no `withLog(...)` wrapper here.
            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toMatch(/Log is not initialized/i);
            expect(createContextMock).not.toHaveBeenCalled();
            expect(getCalls()).toEqual([]);
        });
    });

    describe("generator execution errors", () => {
        test("wraps a synchronous throw with the failing path and message", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([genThrowsPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toContain(
                `Error executing script at ${genThrowsPath}`,
            );
            expect(result.body).toContain("boom-throw");
        });

        test("wraps an async rejection with the failing path and message", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([genRejectsPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toContain(
                `Error executing script at ${genRejectsPath}`,
            );
            expect(result.body).toContain("boom-reject");
        });

        test("stops at the first failing generator and reports it", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(
                makePayload([genThrowsPath, genAPath], false),
            );

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toContain(genThrowsPath);
            // The generator after the failing one never runs.
            expect(getCalls()).toEqual([]);
        });
    });

    describe("script load failure", () => {
        test("returns 500 with the failing path embedded in the message", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness(makePayload([missingPath], false));

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(500);
            expect(result.body).toContain(missingPath);
            expect(result.body).toMatch(/failed to load script/i);
            expect(createContextMock).not.toHaveBeenCalled();
        });
    });

    describe("invalid request payload", () => {
        test("rejects a body that is not an object with 400", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness([genAPath]);

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(400);
            expect(result.body.length).toBeGreaterThan(0);
            expect(createContextMock).not.toHaveBeenCalled();
        });

        test("rejects a payload missing params with 400", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness({ paths: [genAPath] });

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(400);
        });

        test("rejects a payload whose dry_run is not a boolean with 400", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness({
                paths: [genAPath],
                params: { dry_run: "yes" },
            });

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(400);
        });

        test("rejects a payload missing paths with 400", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness({ params: { dry_run: false } });

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(400);
        });

        test("rejects a payload with non-string path entries with 400", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness({
                paths: [genAPath, 42],
                params: { dry_run: false },
            });

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(400);
        });

        test("does not run any generator on a bad payload", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeJsonHarness({ nonsense: true });

            const result = await withLog(() => runService(harness, service));

            expect(result.status).toBe(400);
            expect(getCalls()).toEqual([]);
        });
    });

    describe("malformed request body", () => {
        // `parsePayload` does not guard the `JSON.parse` call, so a malformed
        // body propagates out of `run()` rather than producing a 400. This
        // documents the current behaviour.
        test("propagates a JSON parse error out of run()", async () => {
            const service = new ExecGeneratorScript();
            const harness = makeHarness(TEXT_ENCODER.encode("{not valid json"));

            await expect(
                withLog(() => service.run(harness.context)),
            ).rejects.toThrow();
        });
    });
});
