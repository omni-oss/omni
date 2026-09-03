/**
 * Structured include/exclude globs, end to end through the add-many generator.
 *
 * A generator `files` entry is compiled by the shared glob matcher. There is no
 * `!` negation and no `\!` escape: a leading `!` is an ordinary character, so
 * `!keep.txt` matches a file literally named `!keep.txt` on every platform.
 * Exclusion is expressed only through the object form's `exclude`, which always
 * wins. The add-many action is the smallest consumer to drive it.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

function escaperSpec(files: unknown): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/escaper/generator.omni.yaml": {
                name: "escaper",
                description: "exercises structured include/exclude globs",
                actions: [{ type: "add-many", files }],
            },
        },
        files: {
            ".omni/sources/generator/.keep": "",
            "generators/escaper/!keep.txt": "kept\n",
            "generators/escaper/normal.txt": "normal\n",
            "generators/escaper/other.txt": "other\n",
        },
    };
}

describe("+generator @generator (structured include/exclude globs)", {
    tags: ["generator"],
}, () => {
    it("a leading `!` matches a name that begins with a literal `!`", async () => {
        // No `*`/`**` catch-all: only the literal `!keep.txt` is an include, so
        // a copied `!keep.txt` proves the leading `!` is a literal character and
        // nothing else could have matched it.
        const ws = makeWorkspace(escaperSpec(["!keep.txt", "normal.txt"]));

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "escaper",
                "-o",
                "out",
                "--use-defaults",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/!keep.txt")).toBe("kept\n");
        expect(ws.exists("out/normal.txt")).toBe(true);
        // Listed by neither pattern, so it stays out.
        expect(ws.exists("out/other.txt")).toBe(false);
    });

    it("the object form excludes and `exclude` wins", async () => {
        // `*.txt` includes everything; `exclude` drops `other.txt`.
        const ws = makeWorkspace(
            escaperSpec({ include: ["*.txt"], exclude: ["other.txt"] }),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "escaper",
                "-o",
                "out",
                "--use-defaults",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("out/normal.txt")).toBe(true);
        expect(ws.read("out/!keep.txt")).toBe("kept\n");
        expect(ws.exists("out/other.txt")).toBe(false);
    });
});
