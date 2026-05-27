import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
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
import { describe, expect, test, vi } from "vitest";

import { ExecScript, type LoadedScript } from "./service";

// ──────────────────────────────────────────────────────────────────────────
// Fixtures (reused from loader.spec.ts)
// ──────────────────────────────────────────────────────────────────────────
const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), "__fixtures__");

const namedPath = join(FIXTURES, "named.mjs");
const defaultPath = join(FIXTURES, "default.mjs");
const cjsPath = join(FIXTURES, "cjs.cjs");

// A path that is guaranteed not to resolve.
const missingPath = pathToFileURL(join(FIXTURES, "does-not-exist.mjs")).href;

// ──────────────────────────────────────────────────────────────────────────
// Test harness
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
        "exec-script",
        {},
        events,
        requestError.receiver,
    );
    const response = new PendingResponse(id, responseChannel.sender);

    return {
        context: new ServiceContext(request, response),
        responseFrames: responseChannel.receiver,
    };
}

function makeJsonHarness(body: unknown): Harness {
    return makeHarness(TEXT_ENCODER.encode(JSON.stringify(body)));
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
    service: ExecScript,
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
describe("ExecScript", () => {
    describe("successful execution", () => {
        test("loads a single script and returns SUCCESS", async () => {
            const service = new ExecScript();
            const harness = makeJsonHarness([namedPath]);

            const result = await runService(harness, service);

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            expect(result.body).toBe("");
            expect(result.frames.map((f) => f.type)).toEqual([
                FrameType.RESPONSE_START,
                FrameType.RESPONSE_END,
            ]);
        });

        test("loads multiple scripts and preserves their input order", async () => {
            const seen: string[] = [];
            const service = new ExecScript({
                postImportAll: (modules) => {
                    for (const module of modules) {
                        seen.push(module.path);
                    }
                },
            });

            const harness = makeJsonHarness([namedPath, defaultPath, cjsPath]);
            const result = await runService(harness, service);

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            // Promise.all preserves index order even when the imports race.
            expect(seen).toEqual([namedPath, defaultPath, cjsPath]);
        });

        test("treats an empty paths array as a no-op success", async () => {
            const postImportAll = vi.fn<(m: LoadedScript[]) => void>();
            const service = new ExecScript({ postImportAll });

            const harness = makeJsonHarness([]);
            const result = await runService(harness, service);

            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
            expect(postImportAll).toHaveBeenCalledTimes(1);
            expect(postImportAll).toHaveBeenCalledWith([]);
        });

        test("works without any config", async () => {
            const service = new ExecScript();
            const harness = makeJsonHarness([namedPath]);

            const result = await runService(harness, service);
            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
        });
    });

    describe("postImportAll hook", () => {
        test("is called once with the loaded modules", async () => {
            const postImportAll =
                vi.fn<(m: LoadedScript[]) => void | Promise<void>>();
            const service = new ExecScript({ postImportAll });

            const harness = makeJsonHarness([namedPath, defaultPath]);
            await runService(harness, service);

            expect(postImportAll).toHaveBeenCalledTimes(1);
            const modules = postImportAll.mock.calls[0]?.[0];
            expect(modules).toHaveLength(2);
            expect(modules?.[0]?.path).toBe(namedPath);
            expect(modules?.[1]?.path).toBe(defaultPath);
            // Modules are passed through as-is from loadScript.
            expect(modules?.[0]?.module).toMatchObject({
                greeting: "hello",
                value: 42,
            });
            expect(modules?.[1]?.module).toMatchObject({
                default: { kind: "default-export", id: 1 },
                meta: "side-info",
            });
        });

        test("awaits async hooks before responding", async () => {
            let resolved = false;
            const postImport = vi.fn(async () => {
                await new Promise((r) => setTimeout(r, 5));
                resolved = true;
            });
            const service = new ExecScript({ postImport });

            const harness = makeJsonHarness([namedPath]);
            const result = await runService(harness, service);

            expect(resolved).toBe(true);
            expect(result.status).toBe(Number(ResponseStatusCode.SUCCESS));
        });

        test("returns 500 with the hook's error message if it throws", async () => {
            const service = new ExecScript({
                postImportAll: () => {
                    throw new Error("hook boom");
                },
            });

            const harness = makeJsonHarness([namedPath]);
            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toBe("hook boom");
        });

        test("returns 500 with the hook's rejection message if it rejects", async () => {
            const service = new ExecScript({
                postImportAll: async () => {
                    throw new Error("async hook boom");
                },
            });

            const harness = makeJsonHarness([namedPath]);
            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toBe("async hook boom");
        });
    });

    describe("postImport hook", () => {
        test("is called the same number of times as the amount of loaded modules", async () => {
            const postImport =
                vi.fn<(m: LoadedScript) => void | Promise<void>>();
            const service = new ExecScript({ postImport });

            const harness = makeJsonHarness([namedPath, defaultPath]);
            await runService(harness, service);

            expect(postImport).toHaveBeenCalledTimes(2);
            const modules = postImport.mock.calls
                .map((x) => x[0])
                .toSorted((a, b) => -a.path.localeCompare(b.path, "en"));
            expect(modules).toHaveLength(2);
            expect(modules?.[0]?.path).toBe(namedPath);
            expect(modules?.[1]?.path).toBe(defaultPath);
            // Modules are passed through as-is from loadScript.
            expect(modules?.[0]?.module).toMatchObject({
                greeting: "hello",
                value: 42,
            });
            expect(modules?.[1]?.module).toMatchObject({
                default: { kind: "default-export", id: 1 },
                meta: "side-info",
            });
        });

        test("returns 500 with the hook's error message if it throws", async () => {
            const service = new ExecScript({
                postImport: () => {
                    throw new Error("hook boom");
                },
            });

            const harness = makeJsonHarness([namedPath]);
            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toMatch(/hook boom/);
        });

        test("returns 500 with the hook's rejection message if it rejects", async () => {
            const service = new ExecScript({
                postImport: async () => {
                    throw new Error("async hook boom");
                },
            });

            const harness = makeJsonHarness([namedPath]);
            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toMatch(/async hook boom/);
        });
    });

    describe("invalid request body", () => {
        test("rejects a non-array JSON body with 400", async () => {
            const service = new ExecScript();
            const harness = makeJsonHarness({ not: "an array" });

            const result = await runService(harness, service);

            expect(result.status).toBe(400);
            expect(result.body.length).toBeGreaterThan(0);
        });

        test("rejects an array with non-string elements with 400", async () => {
            const service = new ExecScript();
            const harness = makeJsonHarness([namedPath, 42, defaultPath]);

            const result = await runService(harness, service);
            expect(result.status).toBe(400);
        });

        test("rejects malformed JSON with 400", async () => {
            const service = new ExecScript();
            const harness = makeHarness(TEXT_ENCODER.encode("{not valid json"));

            const result = await runService(harness, service);
            expect(result.status).toBe(400);
            expect(result.body.length).toBeGreaterThan(0);
        });

        test("does not invoke postImport on a bad request", async () => {
            const postImport = vi.fn();
            const service = new ExecScript({ postImport });
            const harness = makeJsonHarness("not an array");

            const result = await runService(harness, service);

            expect(result.status).toBe(400);
            expect(postImport).not.toHaveBeenCalled();
        });
    });

    describe("script load failure", () => {
        test("returns 500 with the failing path embedded in the message", async () => {
            const service = new ExecScript();
            const harness = makeJsonHarness([missingPath]);

            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toContain(missingPath);
            expect(result.body).toMatch(/failed to load script/i);
        });

        test("fails the whole request when one of N paths fails", async () => {
            const postImportAll = vi.fn();
            const service = new ExecScript({ postImportAll });

            const harness = makeJsonHarness([namedPath, missingPath]);

            const result = await runService(harness, service);

            expect(result.status).toBe(500);
            expect(result.body).toContain(missingPath);
            // postImport must not run when any module failed to load.
            expect(postImportAll).not.toHaveBeenCalled();
        });
    });
});
