import { describe, expect, test } from "vitest";
import { globMatches, globToRegExp } from "./glob";

describe("globMatches", () => {
    test("`*` / `?` are wildcards; every other character is literal", () => {
        expect(globMatches("git", "git")).toBe(true);
        expect(globMatches("git", "gitx")).toBe(false);
        expect(globMatches("gi?", "git")).toBe(true);
        expect(globMatches("g*t", "great")).toBe(true);
        // Dots are literal, not regex wildcards.
        expect(globMatches("a.b", "axb")).toBe(false);
    });

    test("is fully anchored (whole string, not a substring)", () => {
        expect(globMatches("example.com", "evil.example.com")).toBe(false);
        expect(globMatches("example.com", "example.com.evil")).toBe(false);
    });

    describe("newline handling matches Rust globset", () => {
        test("`*` (including a deny-all `*`) spans newlines", () => {
            // Rust `globset` treats "\n" as an ordinary character, so a value
            // containing a newline must still be caught by `*`. A JS regex
            // built from `.` without the dotAll flag would let it slip past.
            expect(globMatches("*", "a\nb")).toBe(true);
            expect(globMatches("*", "\n")).toBe(true);
            expect(globMatches("a*b", "a\nb")).toBe(true);
            expect(globMatches("?", "\n")).toBe(true);
        });

        test("a trailing newline is not silently accepted by `$`", () => {
            // `$` in a non-multiline regex also matches just before a trailing
            // "\n", which would let a host named `example.com\n` match the
            // grant `example.com`. The true end-of-string anchor rejects it.
            expect(globMatches("example.com", "example.com\n")).toBe(false);
            expect(globMatches("example.com", "example.com\nevil.com")).toBe(
                false,
            );
        });
    });
});

describe("globToRegExp", () => {
    test("compiles with the dotAll flag", () => {
        expect(globToRegExp("*").flags).toContain("s");
    });
});
