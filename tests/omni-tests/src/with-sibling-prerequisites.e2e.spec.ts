/**
 * A task named in `with` is co-scheduled with the task that names it, and its
 * own prerequisites (`dependencies`) must run first. Each task records itself
 * in `order.log` and fails when a required task has not been recorded yet, so
 * a missing or late prerequisite is a failed run and not just a log line.
 * Every command is `node`, so the specs behave the same on every OS.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const MARK_JS = `
const fs = require("fs");
const path = require("path");
const log = path.join(__dirname, "order.log");
const [name, ...required] = process.argv.slice(2);
const seen = fs.existsSync(log)
    ? fs.readFileSync(log, "utf8").split("\\n").filter(Boolean)
    : [];
const ok = required.every((r) => seen.includes(r));
fs.appendFileSync(log, (ok ? name : name + " MISSING-PREREQ") + "\\n");
process.exit(ok ? 0 : 1);
`;

type Task = Record<string, unknown>;

/** A task that records itself after checking that `requires` ran before it. */
function mark(name: string, requires: string[] = [], extra: Task = {}): Task {
    return { exec: ["node", "../mark.js", name, ...requires], ...extra };
}

interface Case {
    label: string;
    spec: WorkspaceSpec["projects"];
    args: string[];
    /** Tasks that must have run exactly once, `setup` before `build-native`. */
    expected: string[];
}

function toolTasks(extra: Record<string, Task> = {}): Record<string, Task> {
    return {
        setup: mark("setup"),
        "build-native": mark("build-native", ["setup"], {
            dependencies: ["setup"],
        }),
        ...extra,
    };
}

const cases: Case[] = [
    {
        label: "a sibling in the same project",
        spec: {
            app: {
                name: "app",
                tasks: {
                    t: mark("t", [], { with: ["build-native"] }),
                    ...toolTasks(),
                },
            },
        },
        args: ["run", "t", "--no-cache"],
        expected: ["setup", "build-native", "t"],
    },
    {
        label: "a sibling in another project, excluded by `-p app`",
        spec: {
            app: {
                name: "app",
                dependencies: ["tool"],
                tasks: { t: mark("t", [], { with: ["tool#build-native"] }) },
            },
            tool: { name: "tool", tasks: toolTasks() },
        },
        args: ["run", "t", "-p", "app", "--no-cache"],
        expected: ["setup", "build-native", "t"],
    },
    {
        label: "a sibling reached through an alias",
        spec: {
            app: {
                name: "app",
                dependencies: ["tool"],
                tasks: { t: mark("t", [], { with: ["tool#native"] }) },
            },
            tool: {
                name: "tool",
                tasks: toolTasks({ native: { alias: "build-native" } }),
            },
        },
        args: ["run", "t", "-p", "app", "--no-cache"],
        expected: ["setup", "build-native", "t"],
    },
    {
        label: "a sibling reached through ^upstream",
        spec: {
            app: {
                name: "app",
                dependencies: ["tool"],
                tasks: { t: mark("t", [], { with: ["^build-native"] }) },
            },
            tool: { name: "tool", tasks: toolTasks() },
        },
        args: ["run", "t", "-p", "app", "--no-cache"],
        expected: ["setup", "build-native", "t"],
    },
    {
        label: "a sibling whose prerequisites are several levels deep",
        spec: {
            app: {
                name: "app",
                tasks: {
                    t: mark("t", [], { with: ["build-native"] }),
                    s1: mark("setup"),
                    s2: mark("s2", ["setup"], { dependencies: ["s1"] }),
                    s3: mark("s3", ["s2"], { dependencies: ["s2"] }),
                    "build-native": mark(
                        "build-native",
                        ["setup", "s2", "s3"],
                        {
                            dependencies: ["s3"],
                        },
                    ),
                },
            },
        },
        args: ["run", "t", "--no-cache"],
        expected: ["setup", "s2", "s3", "build-native", "t"],
    },
    {
        label: "a prerequisite shared with the selected task's own chain",
        spec: {
            app: {
                name: "app",
                dependencies: ["tool"],
                tasks: {
                    t: mark("t", ["setup"], {
                        dependencies: ["tool#setup"],
                        with: ["tool#build-native"],
                    }),
                },
            },
            tool: { name: "tool", tasks: toolTasks() },
        },
        args: ["run", "t", "-p", "app", "--no-cache"],
        expected: ["setup", "build-native", "t"],
    },
    {
        // Dependents are matched by task name, so the dependent is also `x`.
        label: "a dependent selected with --with-dependents",
        spec: {
            app: {
                name: "app",
                dependencies: ["tool"],
                tasks: {
                    x: mark("app-x", ["x"], {
                        dependencies: ["^x"],
                        with: ["tool#build-native"],
                    }),
                },
            },
            tool: { name: "tool", tasks: toolTasks({ x: mark("x") }) },
        },
        args: ["run", "x", "--with-dependents", "--no-cache"],
        expected: ["x", "app-x", "setup", "build-native"],
    },
];

describe("+run @with (sibling prerequisites)", () => {
    for (const c of cases) {
        it(`runs the prerequisites of ${c.label} first`, async () => {
            const ws = makeWorkspace({
                workspace: { projects: ["**"] },
                projects: c.spec,
                files: { "mark.js": MARK_JS },
            });

            const result = await runOmni(c.args, { cwd: ws.cwd });
            const order = ws.exists("order.log")
                ? ws
                      .read("order.log")
                      .split("\n")
                      .filter((l) => l !== "")
                : [];

            expect(result).toHaveSucceeded();
            expect([...order].sort()).toEqual([...c.expected].sort());
            expect(order.indexOf("setup")).toBeLessThan(
                order.indexOf("build-native"),
            );
            expect(order.indexOf("setup")).toBeGreaterThanOrEqual(0);
        });
    }

    it("does not add siblings or prerequisites with --ignore-dependencies", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        t: mark("t", [], { with: ["build-native"] }),
                        ...toolTasks(),
                    },
                },
            },
            files: { "mark.js": MARK_JS },
        });

        const result = await runOmni(
            ["run", "t", "--ignore-dependencies", "--no-cache"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("order.log").split("\n").filter(Boolean)).toEqual(["t"]);
    });
});
