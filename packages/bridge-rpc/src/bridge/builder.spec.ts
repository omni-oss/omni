import { describe, expect, it, vi } from "vitest";
import { BridgeRpcBuilder } from "./builder";

describe("BridgeRpcBuilder", () => {
    it("should create a BridgeRpc instance with the given handlers", () => {
        const rpc = BridgeRpcBuilder.create({
            onReceive: vi.fn(),
            send: vi.fn(),
        })
            .handler("test/path", async (data: unknown) => {
                return { received: data };
            })
            .handler("test/path2", async (data: unknown) => {
                return { received: data };
            })
            .build();

        expect(rpc.hasHandler("test/path")).toBe(true);
        expect(rpc.hasHandler("test/path2")).toBe(true);
    });
});
