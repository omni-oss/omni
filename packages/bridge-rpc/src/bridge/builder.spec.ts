import { describe, expect, it, vi } from "vitest";
import { BridgeRpcBuilder, DuplicatePathError } from "./builder";

describe("BridgeRpcBuilder", () => {
    it("should create a BridgeRpc instance with the given handlers", () => {
        const rpc = BridgeRpcBuilder.create({
            onReceive: vi.fn(),
            send: vi.fn(),
        })
            .requestHandler("test/path", async (request) => {
                return { received: request.data };
            })
            .requestHandler("test/path2", async (request) => {
                return { received: request.data };
            })
            .streamHandler("test/stream", async (stream) => {
                for await (const data of stream.stream) {
                    console.log(data);
                }
            })
            .build();

        expect(rpc.hasRequestHandler("test/path")).toBe(true);
        expect(rpc.hasRequestHandler("test/path2")).toBe(true);
        expect(rpc.hasStreamHandler("test/stream")).toBe(true);
        // biome-ignore lint/suspicious/noExplicitAny: "allow for testing purposes"
        expect(rpc.hasStreamHandler("test/stream-not-found" as any)).toBe(
            false,
        );
    });

    it("should throw if same path is used for request and stream handlers", () => {
        expect(() => {
            BridgeRpcBuilder.create({
                onReceive: vi.fn(),
                send: vi.fn(),
            })
                .requestHandler("test/path", async (request) => {
                    return { received: request.data };
                })
                .streamHandler("test/path", async (stream) => {
                    for await (const data of stream.stream) {
                        console.log(data);
                    }
                })
                .build();
        }).toThrowError(new DuplicatePathError("test/path"));
    });
});
