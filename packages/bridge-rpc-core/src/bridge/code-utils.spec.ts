import { describe, expect, it, vi } from "vitest";
import { getOrSet } from "./code-utils";

describe("code-utils", () => {
    describe("getOrSet", () => {
        it("should set and return the value when the key is not present", () => {
            const map = new Map<string, number>();
            const result = getOrSet(map, "a", () => 1);

            expect(result).toBe(1);
            expect(map.get("a")).toBe(1);
            expect(map.size).toBe(1);
        });

        it("should return the existing value when the key is already present", () => {
            const map = new Map<string, number>();
            map.set("a", 42);
            const factory = vi.fn(() => 1);

            const result = getOrSet(map, "a", factory);

            expect(result).toBe(42);
            expect(factory).not.toHaveBeenCalled();
            expect(map.size).toBe(1);
        });

        it("should only invoke the factory once per missing key", () => {
            const map = new Map<string, number>();
            const factory = vi.fn(() => 7);

            const first = getOrSet(map, "key", factory);
            const second = getOrSet(map, "key", factory);

            expect(first).toBe(7);
            expect(second).toBe(7);
            expect(factory).toHaveBeenCalledTimes(1);
        });

        it("should support different key types", () => {
            const map = new Map<number, string>();
            const result = getOrSet(map, 1, () => "one");

            expect(result).toBe("one");
            expect(map.get(1)).toBe("one");
        });

        it("should support object values and preserve identity", () => {
            const map = new Map<string, { count: number }>();
            const obj = { count: 0 };

            const first = getOrSet(map, "k", () => obj);
            const second = getOrSet(map, "k", () => ({ count: 99 }));

            expect(first).toBe(obj);
            expect(second).toBe(obj);
            expect(second.count).toBe(0);
        });
    });
});
