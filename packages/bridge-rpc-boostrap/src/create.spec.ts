import { describe, expect, test } from "vitest";
import { createStdioRpcInstance } from "./create";

describe("createStdioRpcInstance", () => {
    test("should create an instance without services", () => {
        const rpc = createStdioRpcInstance();

        expect(rpc).toBeDefined();
    });
});
