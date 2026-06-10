import {
    BridgeRpc,
    type ClientHandle,
    type Headers,
    ResponseStatusCode,
    type Service,
    type ServiceContext,
    StreamTransport,
} from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { describe, expect, test } from "vitest";
import { BridgeRpcSystem } from "./bridge-rpc-system";
import {
    DEFAULT_MAX_CHUNK_SIZE,
    FS_ROUTES,
    joinRoute,
    PARAMETERS_HEADER,
    PROC_ROUTES,
    RETURNS_HEADER,
} from "./options";

/* ------------------------------------------------------------------------- */
/* Test transport / harness                                                   */
/* ------------------------------------------------------------------------- */

/**
 * Minimal in-process server-side router that dispatches by `request.path()`
 * to the supplied handlers. Mirrors what the Rust `Router` would do, just
 * enough to validate the JS-side wiring of `BridgeRpcSystem`.
 */
type Handler = (context: ServiceContext) => Promise<void> | void;

function createRouter(handlers: Record<string, Handler>): Service {
    return {
        run: async (context) => {
            const handler = handlers[context.request.path];
            if (!handler) {
                const start = await context.response.start(
                    ResponseStatusCode.NO_HANDLER_FOR_PATH,
                );
                await start.end();
                return;
            }
            await handler(context);
        },
    };
}

function makeHarness(service: Service) {
    const a = new TransformStream<Uint8Array, Uint8Array>();
    const b = new TransformStream<Uint8Array, Uint8Array>();

    // Client side: doesn't host any service.
    const noOpService: Service = { run: async () => {} };
    const client = new BridgeRpc(
        new StreamTransport({ input: a.readable, output: b.writable }),
        noOpService,
    );
    const server = new BridgeRpc(
        new StreamTransport({ input: b.readable, output: a.writable }),
        service,
    );

    return {
        client: client as ClientHandle,
        start: () => Promise.all([client.start(), server.start()]),
        stop: () => Promise.all([client.stop(), server.stop()]),
    };
}

function harness(
    handlers: Record<string, Handler>,
    action: (client: ClientHandle) => Promise<void>,
) {
    return async () => {
        const router = createRouter(handlers);
        const harness = makeHarness(router);
        try {
            await harness.start();
            await action(harness.client);
        } finally {
            await harness.stop();
        }
    };
}

function getParameters<T>(headers: Headers | undefined): T {
    const value = headers?.[PARAMETERS_HEADER];
    if (value === undefined) {
        throw new Error(`missing \`${PARAMETERS_HEADER}\` header`);
    }
    return value as T;
}

async function respondWithReturns(
    ctx: ServiceContext,
    returns: unknown,
): Promise<void> {
    const start = await ctx.response.start(ResponseStatusCode.SUCCESS, {
        [RETURNS_HEADER]: returns as never,
    });
    await start.end();
}

async function respondEmpty(ctx: ServiceContext): Promise<void> {
    await ctx.response.start(ResponseStatusCode.SUCCESS).then((r) => r.end());
}

async function respondWithBody(
    ctx: ServiceContext,
    body: Uint8Array,
    chunkSize: number,
): Promise<void> {
    let active = await ctx.response.start(ResponseStatusCode.SUCCESS);
    for (let off = 0; off < body.byteLength; off += chunkSize) {
        const end = Math.min(off + chunkSize, body.byteLength);
        active = await active.writeBodyChunk(body.subarray(off, end));
    }
    await active.end();
}

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

const SNAPSHOT_PATH = joinRoute("/proc", PROC_ROUTES.SNAPSHOT);

/** A small handler set for `BridgeRpcSystem.create` to succeed. */
function withSnapshot(
    extra: Record<string, Handler> = {},
): Record<string, Handler> {
    return {
        [SNAPSHOT_PATH]: async (ctx) => {
            await respondWithReturns(ctx, {
                current_dir: "/cwd",
                args: ["argv0"],
                env: { FOO: "bar" },
            });
        },
        ...extra,
    };
}

/* ------------------------------------------------------------------------- */
/* Tests                                                                      */
/* ------------------------------------------------------------------------- */

describe("BridgeRpcSystem", () => {
    test(
        "create populates the proc snapshot",
        harness(withSnapshot(), async (client) => {
            const sys = await BridgeRpcSystem.create(client);
            expect(sys.proc.currentDir()).toBe("/cwd");
            expect(sys.proc.args()).toEqual(["argv0"]);
            expect(sys.proc.env()).toEqual({ FOO: "bar" });
        }),
    );

    test(
        "fs.pathExists puts path in parameters and reads value from response",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.PATH_EXISTS)]: async (ctx) => {
                    const params = getParameters<{ path: string }>(
                        ctx.request.headers,
                    );
                    expect(params.path).toBe("/etc/hosts");
                    await respondWithReturns(ctx, { value: true });
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                await expect(sys.fs.pathExists("/etc/hosts")).resolves.toBe(
                    true,
                );
            },
        ),
    );

    test(
        "fs.readFileAsString reads body and decodes UTF-8",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.READ_FILE_AS_STRING)]: async (
                    ctx,
                ) => {
                    const params = getParameters<{ path: string }>(
                        ctx.request.headers,
                    );
                    expect(params.path).toBe("/note.txt");
                    await respondWithBody(
                        ctx,
                        TEXT_ENCODER.encode("hello world"),
                        DEFAULT_MAX_CHUNK_SIZE,
                    );
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                const text = await sys.fs.readFileAsString("/note.txt");
                expect(text).toBe("hello world");
            },
        ),
    );

    test(
        "fs.writeStringToFile sends path in parameters and content as chunked body",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.WRITE_STRING_TO_FILE)]: async (
                    ctx,
                ) => {
                    const params = getParameters<{ path: string }>(
                        ctx.request.headers,
                    );
                    expect(params.path).toBe("/out.txt");
                    const body = await readBody(ctx.request);
                    expect(TEXT_DECODER.decode(body)).toBe("payload");
                    await respondEmpty(ctx);
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                await sys.fs.writeStringToFile("/out.txt", "payload");
            },
        ),
    );

    test(
        "fs.writeBytesToFile chunks bodies larger than maxChunkSize",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.WRITE_BYTES_TO_FILE)]: async (
                    ctx,
                ) => {
                    // We cannot easily count incoming chunk frames here, but
                    // we can verify the assembled body matches what was
                    // sent. Chunking is exercised on the sender side.
                    const body = await readBody(ctx.request);
                    expect(body.byteLength).toBe(50);
                    expect(body[0]).toBe(0);
                    expect(body[49]).toBe(49);
                    await respondEmpty(ctx);
                },
            }),
            async (client) => {
                // chunk size of 16 forces 4 chunks for a 50-byte payload.
                const sys = await BridgeRpcSystem.create(client, {
                    maxChunkSize: 16,
                });
                const payload = new Uint8Array(50).map((_, i) => i);
                await sys.fs.writeBytesToFile("/blob.bin", payload);
            },
        ),
    );

    test(
        "fs.readDirectory returns entries from response parameters",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.READ_DIRECTORY)]: async (ctx) => {
                    await respondWithReturns(ctx, {
                        entries: ["a.txt", "b.txt"],
                    });
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                const entries = await sys.fs.readDirectory("/anywhere");
                expect(entries).toEqual(["a.txt", "b.txt"]);
            },
        ),
    );

    test(
        "fs.rename sends old_path / new_path in parameters",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.RENAME)]: async (ctx) => {
                    const params = getParameters<{
                        old_path: string;
                        new_path: string;
                    }>(ctx.request.headers);
                    expect(params.old_path).toBe("/from");
                    expect(params.new_path).toBe("/to");
                    await respondEmpty(ctx);
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                await sys.fs.rename("/from", "/to");
            },
        ),
    );

    test(
        "fs.stat decodes StatResponse and exposes FileStat methods",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.STAT)]: async (ctx) => {
                    await respondWithReturns(ctx, {
                        is_file: true,
                        is_directory: false,
                        is_symbolic_link: false,
                        size: 42,
                        // Use a bigint to mimic msgpack's i64 decoding.
                        mtime_ms: 1_700_000_000_000n,
                    });
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                const stat = await sys.fs.stat("/x");
                expect(stat.isFile()).toBe(true);
                expect(stat.isDirectory()).toBe(false);
                expect(stat.isSymbolicLink()).toBe(false);
                expect(stat.size).toBe(42);
                expect(stat.mtime.getTime()).toBe(1_700_000_000_000);
            },
        ),
    );

    test(
        "fs.copy forwards both paths and the options object",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.COPY)]: async (ctx) => {
                    const params = getParameters<{
                        src: string;
                        dest: string;
                        options: { overwrite: boolean; recursive: boolean };
                    }>(ctx.request.headers);
                    expect(params).toEqual({
                        src: "/a",
                        dest: "/b",
                        options: { overwrite: true, recursive: false },
                    });
                    await respondEmpty(ctx);
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                await sys.fs.copy("/a", "/b", { overwrite: true });
            },
        ),
    );

    test(
        "proc.setCurrentDir updates the cached snapshot on success",
        harness(
            withSnapshot({
                [joinRoute("/proc", PROC_ROUTES.SET_CURRENT_DIR)]: async (
                    ctx,
                ) => {
                    const params = getParameters<{ dir: string }>(
                        ctx.request.headers,
                    );
                    expect(params.dir).toBe("/new-cwd");
                    await respondEmpty(ctx);
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                expect(sys.proc.currentDir()).toBe("/cwd");
                await sys.proc.setCurrentDir("/new-cwd");
                expect(sys.proc.currentDir()).toBe("/new-cwd");
            },
        ),
    );

    test(
        "RPC failure surfaces as a thrown error",
        harness(
            withSnapshot({
                [joinRoute("/fs", FS_ROUTES.PATH_EXISTS)]: async (ctx) => {
                    const start = await ctx.response.start(
                        ResponseStatusCode.from(500),
                    );
                    await start.end();
                },
            }),
            async (client) => {
                const sys = await BridgeRpcSystem.create(client);
                await expect(sys.fs.pathExists("/whatever")).rejects.toThrow(
                    /failed with status 500/,
                );
            },
        ),
    );

    test(
        "custom prefixes route requests to the configured paths",
        harness(
            {
                [joinRoute("/api/proc", PROC_ROUTES.SNAPSHOT)]: async (ctx) => {
                    await respondWithReturns(ctx, {
                        current_dir: "/custom",
                        args: [],
                        env: {},
                    });
                },
                [joinRoute("/api/fs", FS_ROUTES.PATH_EXISTS)]: async (ctx) => {
                    await respondWithReturns(ctx, { value: false });
                },
            },
            async (client) => {
                const sys = await BridgeRpcSystem.create(client, {
                    fsPrefix: "/api/fs",
                    procPrefix: "/api/proc",
                });
                expect(sys.proc.currentDir()).toBe("/custom");
                await expect(sys.fs.pathExists("/x")).resolves.toBe(false);
            },
        ),
    );
});
