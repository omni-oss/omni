import { describe, expect, it } from "vitest";
import { RegexNotMatchedError, replaceGroup } from "./string-utils";

describe("replaceGroup", () => {
    it("should replace the first occurrence of a group in a single line string", () => {
        const str = "hello world";
        const regex = /hello (?<name>.*)/;
        const groupName = "name";
        const newValue = "world!";
        const result = replaceGroup(str, regex, groupName, newValue);
        expect(result).toBe("hello world!");
    });

    it("should replace the first occurrence of a group in a multi-line string", () => {
        const str = "hello world\nhi dude\nwhat's up";
        const regex = /hello (?<name>.*)/;
        const groupName = "name";
        const newValue = "world!";
        const result = replaceGroup(str, regex, groupName, newValue);
        expect(result).toBe("hello world!\nhi dude\nwhat's up");
    });

    it("should throw an error if the regex does not match", () => {
        const str = "hello world";
        const regex = /hellp/;
        const groupName = "name";
        const newValue = "world!";
        expect(() => replaceGroup(str, regex, groupName, newValue)).toThrow(
            RegexNotMatchedError,
        );
    });
});
