/**
 * `--with-dependents` finds dependents of the selected tasks through tasks
 * with another name, and adds only the ones whose name was requested.
 *
 * Each task writes its own marker file in `markers/` and fails when the marker
 * of a task it needs does not exist yet. A task that runs only because another
 * one needs it (`mid#test`) and a task that was added as a root look the same
 * in the markers, so the tests also assert on a task that depends on the
 * non-matching intermediate and was not requested (`other#lint`): it must not
 * run. Every command is `node`, so the specs behave the same on every OS.
 */

import { readdirSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni } from "@/harness";

const MARK_JS = `
const fs = require("fs");
const path = require("path");
const markers = path.join(__dirname, "markers");
const [name, ...required] = process.argv.slice(2);
fs.mkdirSync(markers, { recursive: true });
const ok = required.every((r) => fs.existsSync(path.join(markers, r)));
fs.writeFileSync(path.join(markers, ok ? name : name + ".MISSING-PREREQ"), "");
process.exit(ok ? 0 : 1);
`;

type Task = Record<string, unknown>;

function mark(name: string, requires: string[] = [], extra: Task = {}): Task {
    return { exec: ["node", "../mark.js", name, ...requires], ...extra };
}

function workspace() {
    return makeWorkspace({
        workspace: { projects: ["**"] },
        projects: {
            core: {
                name: "core",
                tasks: { build: mark("core-build"), lint: mark("core-lint") },
            },
            // `end#build` depends on `core#build` only through `mid#test`.
            mid: {
                name: "mid",
                dependencies: ["core"],
                tasks: {
                    test: mark("mid-test", ["core-build"], {
                        dependencies: ["^build"],
                    }),
                },
            },
            end: {
                name: "end",
                dependencies: ["mid"],
                tasks: {
                    build: mark("end-build", ["mid-test"], {
                        dependencies: ["mid#test"],
                    }),
                },
            },
            // Depends on the intermediate, has another name: never added.
            other: {
                name: "other",
                dependencies: ["mid"],
                tasks: {
                    lint: mark("other-lint", ["mid-test"], {
                        dependencies: ["mid#test"],
                    }),
                },
            },
        },
        files: { "mark.js": MARK_JS },
    });
}

function markerNames(ws: ReturnType<typeof makeWorkspace>): string[] {
    return ws.exists("markers") ? readdirSync(ws.path("markers")).sort() : [];
}

describe("+run @with-dependents (through tasks with another name)", () => {
    it("`-w` adds a same-name dependent that depends on the task through another task", async () => {
        const ws = workspace();

        const result = await runOmni(
            ["run", "build", "-p", "core", "-w", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // `mid#test` runs as a dependency of `end#build`; `other#lint` is
        // neither requested nor needed, so it does not run.
        expect(markerNames(ws)).toEqual([
            "core-build",
            "end-build",
            "mid-test",
        ]);
    });

    it("a dry run lists the added dependent and not the unrelated task", async () => {
        const ws = workspace();

        const result = await runOmni(
            ["run", "build", "-p", "core", "-w", "-d", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(markerNames(ws)).toEqual([]);
        expect(result.stdout).toContain("'end#build'");
        expect(result.stdout).not.toContain("'other#lint'");
    });

    it("without `-w`, only the selected task runs", async () => {
        const ws = workspace();

        const result = await runOmni(
            ["run", "build", "-p", "core", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(markerNames(ws)).toEqual(["core-build"]);
    });

    it("naming `lint` too also adds `other#lint`", async () => {
        const ws = workspace();

        const result = await runOmni(
            ["run", "build", "lint", "-p", "core", "-w", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(markerNames(ws)).toEqual([
            "core-build",
            "core-lint",
            "end-build",
            "mid-test",
            "other-lint",
        ]);
    });
});
