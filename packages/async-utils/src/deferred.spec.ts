import { describe, expect, it } from "vitest";
import { deferred } from "./deferred";

describe("Deferred", () => {
    it("should resolve", async () => {
        const def = deferred<string>();
        def.resolve("foo");
        await expect(def.promise).resolves.toBe("foo");
    });

    it("should reject", async () => {
        const def = deferred();
        def.reject("foo");
        await expect(def.promise).rejects.toBe("foo");
    });
});
