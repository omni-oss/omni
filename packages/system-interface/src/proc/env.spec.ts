import { describe, expect, test } from "vitest";
import { ObjectEnv } from "./env";

describe("ObjectEnv", () => {
    test("get returns a present value", () => {
        const env = new ObjectEnv({ FOO: "bar" });
        expect(env.get("FOO")).toBe("bar");
    });

    test("get returns null for a missing name", () => {
        const env = new ObjectEnv({ FOO: "bar" });
        expect(env.get("MISSING")).toBeNull();
    });

    test("get returns null for an explicitly-undefined value", () => {
        const env = new ObjectEnv({ FOO: undefined });
        expect(env.get("FOO")).toBeNull();
    });

    test("toObject clones the dictionary (mutation-safe)", () => {
        const raw = { FOO: "bar", BAZ: "qux" };
        const env = new ObjectEnv(raw);
        const snapshot = env.toObject();
        expect(snapshot).toEqual({ FOO: "bar", BAZ: "qux" });
        snapshot.FOO = "mutated";
        // Mutating the snapshot must not affect the backing dictionary.
        expect(env.get("FOO")).toBe("bar");
        expect(raw.FOO).toBe("bar");
    });

    test("toObject omits undefined entries", () => {
        const env = new ObjectEnv({ FOO: "bar", GONE: undefined });
        expect(env.toObject()).toEqual({ FOO: "bar" });
    });
});
