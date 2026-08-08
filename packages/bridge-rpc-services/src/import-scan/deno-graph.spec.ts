import { describe, expect, test } from "vitest";
import { toEntryUrl } from "./deno-graph";

// `deno info --json` only populates a module's `local` path when the entry is
// addressed by URL. A bare absolute path leaves the ROOT module's `local`
// undefined, so it would be dropped from the closure and the confined child
// could not read its own entry script. `toEntryUrl` normalizes filesystem-path
// entries to `file:` URLs while passing real URLs through unchanged.
describe("toEntryUrl (strategy B entry normalization)", () => {
    test("converts an absolute Windows path to a file URL", () => {
        const url = toEntryUrl("C:\\Users\\me\\gen.mjs");
        expect(url.startsWith("file:///")).toBe(true);
        expect(url.toLowerCase()).toContain("gen.mjs");
    });

    test("converts an already-absolute path to a file URL", () => {
        const url = toEntryUrl("/home/me/gen.mjs");
        expect(url.startsWith("file://")).toBe(true);
        expect(url.endsWith("/home/me/gen.mjs")).toBe(true);
    });

    test("passes a file: URL through unchanged", () => {
        const input = "file:///C:/Users/me/gen.mjs";
        expect(toEntryUrl(input)).toBe(input);
    });

    test("passes npm:/jsr:/http(s): specifiers through unchanged", () => {
        expect(toEntryUrl("npm:is-number@7")).toBe("npm:is-number@7");
        expect(toEntryUrl("jsr:@std/path")).toBe("jsr:@std/path");
        expect(toEntryUrl("https://example.com/mod.ts")).toBe(
            "https://example.com/mod.ts",
        );
    });
});
