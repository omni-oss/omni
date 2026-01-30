import { describe, expect, it } from "vitest";
import { NoValueAtPathError, setValue, setValueIn } from "./set-value";

describe.each([
    {
        name: "setValue",
        fn: setValue,
    },
    {
        name: "setValueIn",
        fn: setValueIn,
    },
])("$name", ({ fn: set }) => {
    it("should set a value at a path of object", () => {
        const objectGraph = {
            foo: {
                bar: {
                    baz: 42,
                },
            },
        };

        const returned = set(objectGraph, ["foo", "bar", "baz"], 43);

        expect(returned).toEqual({
            foo: {
                bar: {
                    baz: 43,
                },
            },
        });
    });

    it("should set a value at a path of array", () => {
        const objectGraph = {
            foo: [
                {
                    bar: {
                        baz: 42,
                    },
                },
            ] as const,
        };

        const returned = set(objectGraph, ["foo", 0, "bar", "baz"], 43);

        expect(returned).toEqual({
            foo: [
                {
                    bar: {
                        baz: 43,
                    },
                },
            ],
        });
    });

    it("should return the value if the path is empty", () => {
        const objectGraph = {
            foo: {
                bar: {
                    baz: 42,
                },
            },
        };

        const returned = set(objectGraph, [], 43);

        expect(returned).toEqual(43);
    });

    it("should throw if the path is invalid", () => {
        const objectGraph = {
            foo: {
                bar: {
                    baz: 42,
                },
            },
        };

        expect(() =>
            // biome-ignore lint/suspicious/noExplicitAny: expected runtime code for testing
            set(objectGraph, ["foo", "bar", "baz", "qux"] as any, 43),
        ).toThrow(NoValueAtPathError);
    });
});
