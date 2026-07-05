import { describe, expect, it } from "vitest";
import { formatVersionList, pad, renderTable } from "./format";

describe("pad", () => {
    it("right-pads to a width and leaves longer strings alone", () => {
        expect(pad("ab", 5)).toBe("ab   ");
        expect(pad("abcdef", 3)).toBe("abcdef");
    });
});

describe("renderTable", () => {
    it("auto-sizes columns and emits header + separator + rows", () => {
        const lines = renderTable(
            ["tool", "warm"],
            [
                ["omni", "50ms"],
                ["turbo", "100ms"],
            ],
        );
        expect(lines).toEqual([
            "| tool  | warm  |",
            "| ----- | ----- |",
            "| omni  | 50ms  |",
            "| turbo | 100ms |",
        ]);
    });
});

describe("formatVersionList", () => {
    it("joins tool/version pairs with a prefix and ? for unknowns", () => {
        expect(
            formatVersionList(
                [
                    ["omni", "0.17.1"],
                    ["turbo", null],
                ],
                "versions",
            ),
        ).toBe("versions: omni 0.17.1, turbo ?");
    });
});
