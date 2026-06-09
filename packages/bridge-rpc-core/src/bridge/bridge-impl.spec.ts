import { describe, expect, it, vi } from "vitest";
import { Id } from "@/id";
import type { Transport } from "@/transport";
import { BridgeRpc } from "./bridge-impl";
import { decodeFrame, encodeFrame } from "./codec-utils";
import { Frame } from "./frame";
import { FrameType } from "./frame-schema";
import type { ServiceContext } from "./service";
import { ResponseStatusCode } from "./status-code";

// Mock Constants
const TEST_DATA = "test_data";
const TEST_PATH = "test_path";
const testDataBytes = new TextEncoder().encode(TEST_DATA);

describe("BridgeRpc", () => {
    // Helper to create a Mock Transport
    function createMockTransport() {
        const mockTransport = {
            send: vi.fn(),
            onReceive: vi.fn(),
        } satisfies Transport;

        const onReceiveHandlers = [] as ((
            data: Uint8Array,
        ) => void | Promise<void>)[];

        mockTransport.onReceive.mockImplementation((cb) => {
            onReceiveHandlers.push(cb);
        });

        return {
            onReceiveHandlers,
            ...mockTransport,
            sendToHandlers: async (data: Uint8Array) => {
                for (const handler of onReceiveHandlers) {
                    await handler(data);
                }
            },
        };
    }

    // Helper to create a Mock Service
    const createMockService = () => ({
        run: vi.fn().mockImplementation(async (ctx: ServiceContext) => {
            const { request, response } = ctx;
            const requestBytes = await readAll(request.readBody());

            const activeResponse = await response.start(
                ResponseStatusCode.SUCCESS,
            );
            await activeResponse.writeBodyChunk(requestBytes);
            await activeResponse.end();

            return undefined;
        }),
    });

    const sleep = (ms: number) =>
        new Promise((resolve) => setTimeout(resolve, ms));

    it("should send close frame when stopped", async () => {
        const transport = createMockTransport();
        const service = createMockService();
        const rpc = new BridgeRpc(transport, service);

        await expect(rpc.start()).resolves.toBeUndefined();
        await expect(rpc.stop()).resolves.toBeUndefined();

        expect(transport.send).toHaveBeenCalled();
        // biome-ignore lint/style/noNonNullAssertion: should have value
        const lastCallBytes = transport.send.mock.calls[0]![0];
        const decodedFrame = decodeFrame(lastCallBytes);

        expect(decodedFrame).toEqual(Frame.close());
    });

    it("should handle ping/pong cycle", async () => {
        const transport = createMockTransport();
        const service = createMockService();
        const rpc = new BridgeRpc(transport, service);

        // When RPC sends a PING, we simulate the other side sending a PONG back
        transport.send.mockImplementation(async (bytes: Uint8Array) => {
            const frame = decodeFrame(bytes);
            if (frame.type === FrameType.PING) {
                // Simulate network delay then send PONG
                const pongFrame = Frame.pong();
                const encoded = encodeFrame(pongFrame);
                await transport.sendToHandlers(encoded);
                await sleep(1);
            }
        });

        await expect(rpc.start()).resolves.toBeUndefined();
        // This should resolve when the PONG is received via transport.onReceive
        await expect(rpc.ping(1000)).resolves.toBeTruthy();

        await expect(rpc.stop()).resolves.toBeUndefined();
    });

    it("client sending request and receiving response", async () => {
        const transport = createMockTransport();
        const service = createMockService();
        const rpc = new BridgeRpc(transport, service);
        const reqId = Id.create();

        await rpc.start();

        // 1. Setup transport to simulate a server response when it receives request frames
        transport.send.mockImplementation(async (bytes: Uint8Array) => {
            const frame = decodeFrame(bytes);

            // If we see the end of the request, simulate the server's response
            if (frame.type === FrameType.REQUEST_END) {
                const frames = [
                    Frame.responseStart(reqId, ResponseStatusCode.SUCCESS),
                    Frame.responseBodyChunk(reqId, testDataBytes),
                    Frame.responseEnd(reqId),
                ];

                for (const frame of frames) {
                    const encoded = encodeFrame(frame);
                    await transport.sendToHandlers(encoded);
                    await sleep(1); // simulate network delay
                }
            }
        });

        const pendingRequest = await rpc.requestWithId(reqId, TEST_PATH);
        const requestHandle = await pendingRequest.start();
        await requestHandle.writeBodyChunk(testDataBytes);
        const response = await requestHandle.end().then((x) => x.wait());

        expect(response.status).toEqual(ResponseStatusCode.SUCCESS);
        const body = await readAll(response.readBody());

        expect(body).toEqual(testDataBytes);
        await expect(rpc.stop()).resolves.toBeUndefined();
    });

    it("server handling request and sending response", async () => {
        const transport = createMockTransport();
        const reqId = Id.create();

        // Logic inside the mock service to handle the incoming request
        const service = {
            run: vi.fn().mockImplementation(async (ctx: ServiceContext) => {
                const { request, response } = ctx;
                // Read the request body
                const body = await readAll(request.readBody());

                // Send back response
                const respWriter = await response.start(
                    ResponseStatusCode.SUCCESS,
                );

                await respWriter.writeBodyChunk(body);
                await respWriter.end();

                expect(request.path).toBe(TEST_PATH);
            }),
        };

        const rpc = new BridgeRpc(transport, service);
        await rpc.start();

        const frames = [
            Frame.requestStart(reqId, TEST_PATH),
            Frame.requestBodyChunk(reqId, testDataBytes),
            Frame.requestEnd(reqId),
        ];

        for (const frame of frames) {
            const encoded = encodeFrame(frame);
            await transport.sendToHandlers(encoded);
        }

        // Allow microtasks to process (service.run is called in background)
        await sleep(50);

        // Verify transport sent response frames back
        const sentFrameTypes = transport.send.mock.calls.map(
            (call) => decodeFrame(call[0]).type,
        );

        expect(sentFrameTypes[0]).toEqual(FrameType.RESPONSE_START);
        expect(sentFrameTypes[1]).toEqual(FrameType.RESPONSE_BODY_CHUNK);
        expect(sentFrameTypes[2]).toEqual(FrameType.RESPONSE_END);

        await expect(rpc.stop()).resolves.toBeUndefined();
    });
});

async function readAll(gen: AsyncIterable<Uint8Array>): Promise<Uint8Array> {
    const chunks: Uint8Array[] = [];
    for await (const chunk of gen) {
        chunks.push(chunk);
    }
    const wholeChunk = new Uint8Array(
        chunks.reduce((acc, c) => acc + c.length, 0),
    );

    for (const chunk of chunks) {
        wholeChunk.set(chunk, wholeChunk.length - chunk.length);
    }

    return wholeChunk;
}
