import { describe, expect, it } from "vitest";
import { BridgeRpc } from "@/bridge";
import { decode, decodeFrame, encode, encodeFrame } from "@/bridge/codec-utils";
import { ResponseErrorCode } from "@/bridge/error-code";
import { Frame, FrameType } from "@/bridge/frame";
import { FrameSchema } from "@/bridge/frame-schema";
import type { ServiceContext } from "@/bridge/service";
import { ResponseStatusCode } from "@/bridge/status-code";
import { Id } from "@/id";
import { delay } from "@/promise-utils";
import type { Transport } from "@/transport";
import { StreamTransport } from "@/transport";

// ---------------------------------------------------------------------------
// Module-level helpers (shared by all describe blocks)
// ---------------------------------------------------------------------------

async function readAll(gen: AsyncIterable<Uint8Array>): Promise<Uint8Array> {
    const chunks: Uint8Array[] = [];
    for await (const chunk of gen) {
        chunks.push(chunk);
    }
    const wholeChunk = new Uint8Array(
        chunks.reduce((acc, c) => acc + c.length, 0),
    );

    let offset = 0;
    for (const chunk of chunks) {
        wholeChunk.set(chunk, offset);
        offset += chunk.length;
    }

    return wholeChunk;
}

function createTestService() {
    return {
        run: async (context: ServiceContext) => {
            const { request, response } = context;

            const requestBody = await readAll(request.readBody());

            const activeResponse = await response.start(
                ResponseStatusCode.SUCCESS,
            );
            await activeResponse.writeBodyChunk(requestBody);
            await activeResponse.end();
        },
    };
}

/**
 * A minimal transport implementation for tests that need to inject raw
 * frames or inspect what the RPC sends.
 *
 * – `injectFrame(frame)` simulates receiving a frame from the remote peer.
 * – `sentFrames` accumulates frames that the RPC sends out.
 */
class TestTransport implements Transport {
    private callbacks: Array<(data: Uint8Array) => void | Promise<void>> = [];
    readonly sentFrames: Frame[] = [];

    async send(data: Uint8Array): Promise<void> {
        // The FrameTransporter already encodes the frame with encodeFrame
        // before calling transport.send(); decode it back so we can inspect it.
        try {
            const decoded = decodeFrame(data);
            const parsed = FrameSchema.safeParse(decoded);
            if (parsed.success) {
                this.sentFrames.push(parsed.data);
            }
        } catch {
            // ignore unparseable frames in tests
        }
    }

    onReceive(callback: (data: Uint8Array) => void | Promise<void>): void {
        this.callbacks.push(callback);
    }

    /** Inject a frame as if it arrived from the remote peer. */
    async injectFrame(frame: Frame): Promise<void> {
        const bytes = encodeFrame(frame);
        for (const cb of this.callbacks) {
            await cb(bytes);
        }
    }

    /** Wait until at least `count` frames have been sent, with a timeout. */
    async waitForSentFrames(count: number, timeoutMs = 500): Promise<void> {
        const deadline = Date.now() + timeoutMs;
        while (this.sentFrames.length < count && Date.now() < deadline) {
            await delay(10);
        }
        if (this.sentFrames.length < count) {
            throw new Error(
                `Timed out waiting for ${count} sent frames; only got ${this.sentFrames.length}`,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Existing integration tests
// ---------------------------------------------------------------------------

describe("Rpc to Rpc Integration", () => {
    type Rpcs = {
        rpc1: BridgeRpc;
        rpc2: BridgeRpc;
        start: () => Promise<void>;
        stop: () => Promise<void>;
    };

    function createRpcs(): Rpcs {
        const transport1Side = new TransformStream<Uint8Array, Uint8Array>();
        const transport2Side = new TransformStream<Uint8Array, Uint8Array>();

        const transport1 = new StreamTransport({
            input: transport1Side.readable,
            output: transport2Side.writable,
        });

        const service1 = createTestService();

        const rpc1 = new BridgeRpc(transport1, service1);

        const transport2 = new StreamTransport({
            input: transport2Side.readable,
            output: transport1Side.writable,
        });
        const service2 = createTestService();
        const rpc2 = new BridgeRpc(transport2, service2);

        const ret = {
            rpc1,
            rpc2,
            start: async () => {
                await Promise.all([rpc1.start(), rpc2.start()]);
            },
            stop: async () => {
                await Promise.all([rpc1.stop(), rpc2.stop()]);
            },
        };

        return ret;
    }

    function withRpcs(
        action: (rpc1: BridgeRpc, rpc2: BridgeRpc) => Promise<void>,
    ) {
        return async () => {
            const { rpc1, rpc2, start, stop } = createRpcs();
            try {
                await start();
                await delay(10);
                await action(rpc1, rpc2);
                await delay(10);
            } finally {
                await stop();
            }
        };
    }

    it(
        "should be able to handle ping/pong cycle",
        withRpcs(async (rpc) => {
            const pingResult = await rpc.ping(100);

            expect(pingResult).toBe(true);
        }),
    );

    it(
        "should be able to send and receive data",
        withRpcs(async (rpc) => {
            const reqData = { test: "test" };
            const reqDataBytes = encode(reqData);

            const activeRequest = await (await rpc.request("rpc2test")).start();

            await activeRequest.writeBodyChunk(reqDataBytes);
            const response = await activeRequest.end();
            const activeResponse = await response.wait();
            const responseBody = await readAll(activeResponse.readBody());

            expect(activeResponse.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(decode(responseBody)).toEqual(reqData);
        }),
    );
});

// ---------------------------------------------------------------------------
// Error-recovery tests
// ---------------------------------------------------------------------------

describe("Error recovery", () => {
    it("should send ResponseError and keep running when it receives a frame for an unknown session", async () => {
        const transport = new TestTransport();
        const service = createTestService();
        const rpc = new BridgeRpc(transport, service);

        await rpc.start();
        await delay(5);

        const unknownId = Id.create();

        // Inject a RequestBodyChunk for a session that was never started.
        await transport.injectFrame(
            Frame.requestBodyChunk(unknownId, new Uint8Array([1, 2, 3])),
        );

        // The RPC should have sent back a ResponseError with UNEXPECTED_FRAME.
        await transport.waitForSentFrames(1);
        const sentFrame = transport.sentFrames[0];
        expect(sentFrame).toBeDefined();
        if (sentFrame?.type === FrameType.RESPONSE_ERROR) {
            expect(sentFrame.data.id.equals(unknownId)).toBe(true);
            expect(
                sentFrame.data.code.equals(ResponseErrorCode.UNEXPECTED_FRAME),
            ).toBe(true);
        } else {
            expect(sentFrame?.type).toBe(FrameType.RESPONSE_ERROR);
        }

        // The RPC must still be running (inject a close to stop it cleanly).
        await transport.injectFrame(Frame.close());
        await rpc.stop();
    });

    it("should send ResponseError and keep running when it receives out-of-order frames", async () => {
        const transport = new TestTransport();
        const service = createTestService();
        const rpc = new BridgeRpc(transport, service);

        await rpc.start();
        await delay(5);

        const id = Id.create();

        // Start a valid session.
        await transport.injectFrame(Frame.requestStart(id, "test/path"));

        // Immediately inject a second RequestStart for the same ID.
        // The state machine is in the Started state; this is invalid.
        await transport.injectFrame(Frame.requestStart(id, "test/path"));

        // Expect a ResponseError.
        await transport.waitForSentFrames(1);
        const sentFrame = transport.sentFrames[0];
        expect(sentFrame).toBeDefined();
        if (sentFrame?.type === FrameType.RESPONSE_ERROR) {
            expect(sentFrame.data.id.equals(id)).toBe(true);
            expect(
                sentFrame.data.code.equals(ResponseErrorCode.UNEXPECTED_FRAME),
            ).toBe(true);
        } else {
            expect(sentFrame?.type).toBe(FrameType.RESPONSE_ERROR);
        }

        // RPC must still be running.
        await transport.injectFrame(Frame.close());
        await rpc.stop();
    });

    it("should continue handling valid requests after recovering from a bad frame", async () => {
        const transport1Side = new TransformStream<Uint8Array, Uint8Array>();
        const transport2Side = new TransformStream<Uint8Array, Uint8Array>();

        const transport1 = new StreamTransport({
            input: transport1Side.readable,
            output: transport2Side.writable,
        });
        const service1 = createTestService();
        const rpc1 = new BridgeRpc(transport1, service1);

        const transport2 = new StreamTransport({
            input: transport2Side.readable,
            output: transport1Side.writable,
        });
        const service2 = createTestService();
        const rpc2 = new BridgeRpc(transport2, service2);

        await rpc1.start();
        await rpc2.start();
        await delay(10);

        // Send a normal request first to confirm the link is working.
        const reqData = { test: "hello" };
        const reqDataBytes = encode(reqData);
        const activeRequest1 = await (await rpc1.request("path")).start();
        await activeRequest1.writeBodyChunk(reqDataBytes);
        const response1 = await activeRequest1.end();
        const activeResponse1 = await response1.wait();
        const body1 = await readAll(activeResponse1.readBody());
        expect(decode(body1)).toEqual(reqData);

        // Now send a normal request from rpc2 to verify rpc1 (as a server)
        // still handles things correctly.
        const reqData2 = { test: "world" };
        const reqDataBytes2 = encode(reqData2);
        const activeRequest2 = await (await rpc2.request("path")).start();
        await activeRequest2.writeBodyChunk(reqDataBytes2);
        const response2 = await activeRequest2.end();
        const activeResponse2 = await response2.wait();
        const body2 = await readAll(activeResponse2.readBody());
        expect(decode(body2)).toEqual(reqData2);

        await delay(10);
        await rpc1.stop();
        await rpc2.stop();
    });
});

// ---------------------------------------------------------------------------
// Production-readiness scenarios
// ---------------------------------------------------------------------------

describe("Production-readiness scenarios", () => {
    // -------------------------------------------------------------------
    // Helpers shared only by this block
    // -------------------------------------------------------------------

    /** Create a pair of cross-connected StreamTransport RPCs. */
    function createRpcs() {
        const t1 = new TransformStream<Uint8Array, Uint8Array>();
        const t2 = new TransformStream<Uint8Array, Uint8Array>();
        const rpc1 = new BridgeRpc(
            new StreamTransport({ input: t1.readable, output: t2.writable }),
            createTestService(),
        );
        const rpc2 = new BridgeRpc(
            new StreamTransport({ input: t2.readable, output: t1.writable }),
            createTestService(),
        );
        return { rpc1, rpc2 };
    }

    async function roundTrip(
        rpc: BridgeRpc,
        payload: Uint8Array,
    ): Promise<Uint8Array> {
        const active = await (await rpc.request("path")).start();
        await active.writeBodyChunk(payload);
        const pending = await active.end();
        const resp = await pending.wait();
        return readAll(resp.readBody());
    }

    // -------------------------------------------------------------------
    // 1. Concurrent requests
    // -------------------------------------------------------------------
    it("concurrent requests all complete with correct data", async () => {
        const { rpc1, rpc2 } = createRpcs();
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        const N = 100;
        const results = await Promise.all(
            Array.from({ length: N }, async (_, i) => {
                const payload = encode({ index: i, msg: `payload_${i}` });
                const received = await roundTrip(rpc1, payload);
                return { i, received };
            }),
        );

        for (const { i, received } of results) {
            expect(decode(received)).toEqual({
                index: i,
                msg: `payload_${i}`,
            });
        }

        await rpc1.stop();
        await rpc2.stop();
    }, 10_000);

    // -------------------------------------------------------------------
    // 2. Bidirectional simultaneous requests
    // -------------------------------------------------------------------
    it("bidirectional simultaneous requests both complete correctly", async () => {
        const { rpc1, rpc2 } = createRpcs();
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        const d1 = encode({ from: "rpc1" });
        const d2 = encode({ from: "rpc2" });

        const [r1, r2] = await Promise.all([
            roundTrip(rpc1, d1),
            roundTrip(rpc2, d2),
        ]);

        expect(decode(r1)).toEqual({ from: "rpc1" });
        expect(decode(r2)).toEqual({ from: "rpc2" });

        await rpc1.stop();
        await rpc2.stop();
    }, 5_000);

    // -------------------------------------------------------------------
    // 3. Multi-chunk body streaming
    // -------------------------------------------------------------------
    it("multi-chunk body is streamed and received in full", async () => {
        const { rpc1, rpc2 } = createRpcs();
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        const NUM_CHUNKS = 5;
        const CHUNK_SIZE = 128;
        // Each chunk has a distinct fill byte so we can detect corruption.
        const chunks = Array.from({ length: NUM_CHUNKS }, (_, i) =>
            new Uint8Array(CHUNK_SIZE).fill(i),
        );
        const expected = new Uint8Array(NUM_CHUNKS * CHUNK_SIZE);
        for (let i = 0; i < NUM_CHUNKS; i++) {
            // biome-ignore lint/style/noNonNullAssertion: test code, we know these are all defined.
            expected.set(chunks[i]!, i * CHUNK_SIZE);
        }

        const active = await (await rpc1.request("path")).start();
        for (const chunk of chunks) {
            await active.writeBodyChunk(chunk);
        }
        const resp = await (await active.end()).wait();
        const received = await readAll(resp.readBody());

        expect(received).toEqual(expected);

        await rpc1.stop();
        await rpc2.stop();
    }, 5_000);

    // -------------------------------------------------------------------
    // 4. Empty body request and response
    // -------------------------------------------------------------------
    it("empty body request and response work correctly", async () => {
        const emptyService = {
            run: async (ctx: ServiceContext) => {
                // drain any body (none expected)
                await readAll(ctx.request.readBody());
                const active = await ctx.response.start(
                    ResponseStatusCode.SUCCESS,
                );
                await active.end();
            },
        };

        const t1 = new TransformStream<Uint8Array, Uint8Array>();
        const t2 = new TransformStream<Uint8Array, Uint8Array>();
        const rpc1 = new BridgeRpc(
            new StreamTransport({ input: t1.readable, output: t2.writable }),
            createTestService(),
        );
        const rpc2 = new BridgeRpc(
            new StreamTransport({ input: t2.readable, output: t1.writable }),
            emptyService,
        );
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        // No body chunks — start then end immediately.
        const active = await (await rpc1.request("path")).start();
        const resp = await (await active.end()).wait();

        expect(resp.status).toEqual(ResponseStatusCode.SUCCESS);

        // Consume the empty body — must not hang or throw.
        const body = await readAll(resp.readBody());
        expect(body.length).toBe(0);

        await rpc1.stop();
        await rpc2.stop();
    }, 5_000);

    // -------------------------------------------------------------------
    // 5. Large payload round-trip
    // -------------------------------------------------------------------
    it("large payload is transmitted without loss or corruption", async () => {
        const { rpc1, rpc2 } = createRpcs();
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        // 128 KB with a deterministic pattern (i % 251) so we can
        // detect both truncation and bit-flip.
        const SIZE = 128 * 1024;
        const large = new Uint8Array(SIZE);
        for (let i = 0; i < SIZE; i++) large[i] = i % 251;

        const received = await roundTrip(rpc1, large);

        expect(received.length).toBe(large.length);
        expect(received).toEqual(large);

        await rpc1.stop();
        await rpc2.stop();
    }, 10_000);

    // -------------------------------------------------------------------
    // 6. Custom response status code
    // -------------------------------------------------------------------
    it("custom response status code is preserved across the wire", async () => {
        const NO_HANDLER = ResponseStatusCode.NO_HANDLER_FOR_PATH;

        const customStatusService = {
            run: async (ctx: ServiceContext) => {
                await readAll(ctx.request.readBody());
                const active = await ctx.response.start(NO_HANDLER);
                await active.end();
            },
        };

        const t1 = new TransformStream<Uint8Array, Uint8Array>();
        const t2 = new TransformStream<Uint8Array, Uint8Array>();
        const rpc1 = new BridgeRpc(
            new StreamTransport({ input: t1.readable, output: t2.writable }),
            createTestService(),
        );
        const rpc2 = new BridgeRpc(
            new StreamTransport({ input: t2.readable, output: t1.writable }),
            customStatusService,
        );
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        const active = await (await rpc1.request("path")).start();
        const resp = await (await active.end()).wait();

        expect(resp.status.equals(NO_HANDLER)).toBe(true);

        await rpc1.stop();
        await rpc2.stop();
    }, 5_000);

    // -------------------------------------------------------------------
    // 7. Many sequential requests
    // -------------------------------------------------------------------
    it("many sequential requests all complete successfully", async () => {
        const { rpc1, rpc2 } = createRpcs();
        await rpc1.start();
        await rpc2.start();
        await delay(10);

        const N = 100;
        for (let i = 0; i < N; i++) {
            const payload = encode({ seq: i });
            const received = await roundTrip(rpc1, payload);
            expect(decode(received)).toEqual({ seq: i });
        }

        await rpc1.stop();
        await rpc2.stop();
    }, 15_000);
});
