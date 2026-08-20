import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { Id, ServiceContext } from "@omni-oss/bridge-rpc-core";
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

// Stub the context factory: the real one reaches over RPC to build a System.
const { createContextMock } = vi.hoisted(() => ({
    createContextMock: vi.fn(),
}));

vi.mock("./tool-script-context", () => ({
    DefaultToolScriptContext: {
        create: createContextMock,
    },
}));

import { ExecTool } from "./service";

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), "__fixtures__");
const returnsValuePath = join(FIXTURES, "returns-value.mjs");
const returnsAsyncPath = join(FIXTURES, "returns-async.mjs");
const returnsUndefinedPath = join(FIXTURES, "returns-undefined.mjs");
const throwsPath = join(FIXTURES, "throws.mjs");
const noDefaultPath = join(FIXTURES, "no-default.mjs");
const nonFnDefaultPath = join(FIXTURES, "non-fn-default.mjs");
const missingPath = pathToFileURL(join(FIXTURES, "does-not-exist.mjs")).href;

const TEST_LOGGER = { id: "test-logger" } as unknown as Logger;
const TEST_FACTORY: LoggerFactory = { get: () => TEST_LOGGER };

function withLog<T>(fn: () => Promise<T>): Promise<T> {
    return Log.withRoot(TEST_FACTORY, ["test"], fn);
}

const SYS_MARKER = { __marker: "sys" };

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
        "exec-tool",
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

type ResponseSummary = { status: number; body: string; frames: Frame[] };

async function runService(
    harness: Harness,
    service: ExecTool,
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

describe("ExecTool", () => {
    beforeEach(() => {
        createContextMock.mockReset();
        createContextMock.mockImplementation(
            async (options: { inputs: unknown; cwd: string }) => ({
                inputs: options.inputs,
                sys: SYS_MARKER,
                log: TEST_LOGGER,
            }),
        );
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    test("captures the return value into the response body", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: returnsValuePath,
            cwd: "/ws",
            inputs: { who: "world" },
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(0);
        expect(JSON.parse(result.body)).toEqual({
            echoed: { who: "world" },
            count: 2,
            hasSys: true,
            hasLog: true,
        });
    });

    test("awaits an async tool before responding", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: returnsAsyncPath,
            cwd: "/ws",
            inputs: "me",
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(0);
        expect(JSON.parse(result.body)).toEqual({ ok: true, who: "me" });
    });

    test("serializes an undefined return as null", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: returnsUndefinedPath,
            cwd: "/ws",
            inputs: {},
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(0);
        expect(result.body).toBe("null");
    });

    test("fails with 500 when the tool throws", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: throwsPath,
            cwd: "/ws",
            inputs: {},
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(500);
    });

    test("fails with 500 when there is no default export", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: noDefaultPath,
            cwd: "/ws",
            inputs: {},
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(500);
    });

    test("fails with 500 when the default export is not a function", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: nonFnDefaultPath,
            cwd: "/ws",
            inputs: {},
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(500);
    });

    test("fails with 500 when the script cannot be loaded", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({
            path: missingPath,
            cwd: "/ws",
            inputs: {},
        });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(500);
    });

    test("rejects a payload missing path with 400", async () => {
        const service = new ExecTool();
        const harness = makeJsonHarness({ inputs: {} });

        const result = await withLog(() => runService(harness, service));

        expect(result.status).toBe(400);
    });
});
