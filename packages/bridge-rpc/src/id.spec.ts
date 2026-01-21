import { describe, expect, it } from "vitest";
import { Id } from "./id";

describe("Id", () => {
    it("should be able to create an Id", () => {
        const id = Id.create();
        expect(id.getValue()).toBeTypeOf("bigint");
    });

    // the javascript implementation is very slow
    // so we need to run this test for a long time to not timeout the test
    // also limited only to 50_000 iterations since the usual 1_000_000 test is slow on js
    it(
        "should never be equal to another Id",
        {
            timeout: 10_000,
        },
        () => {
            const keys = new Set<bigint>();
            for (let i = 0; i < 50_000; i++) {
                const id = Id.create();
                expect(keys.has(id.getValue())).toBe(false);
                keys.add(id.getValue());
            }
        },
    );
});
