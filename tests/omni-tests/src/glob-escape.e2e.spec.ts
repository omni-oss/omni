/**
 * The `\!` glob escape, end to end through the add-many generator.
 *
 * A generator `files` entry is compiled by the shared glob matcher, so a leading
 * `!` means exclude and `\!` means "match a name that begins with a literal `!`".
 * This must hold on every platform, which is the whole reason the escape is
 * resolved before globset (globset's own backslash escape is off on Windows).
 * The add-many action is the smallest negation-aware consumer to drive it.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

function escaperSpec(files: string[]): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/escaper/generator.omni.yaml": {
                name: "escaper",
                description: "exercises the backslash-bang escape",
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

describe("+generator @generator (backslash-bang glob escape)", {
    tags: ["generator"],
}, () => {
    it("`\\!` includes a name that begins with a literal `!`", async () => {
        // No `*`/`**` catch-all: only the escaped literal is an include, so a
        // copied `!keep.txt` is proof the escape fired and nothing else could
        // have matched it.
        const ws = makeWorkspace(escaperSpec(["\\!keep.txt", "normal.txt"]));

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

    it("an unescaped leading `!` still excludes (parity is undisturbed)", async () => {
        // `*.txt` includes everything; `!other.txt` excludes `other.txt`.
        const ws = makeWorkspace(escaperSpec(["*.txt", "!other.txt"]));

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
