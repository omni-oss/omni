/**
 * `omni tool` - the tool subsystem end to end: discovery via the workspace
 * `tools:` sources, `omni tool list` / `inspect`, and `omni tool run` spawning
 * the vendored JS bridge to execute a `type: js` tool and capture its return
 * value as JSON. Pinned to `crates/omni_tool/*`, `crates/omni_api/src/operations/tool.rs`,
 * and `crates/omni_cli_core/src/commands/tool.rs`.
 *
 * These tests require the `deno` runtime on PATH (tools default to `deno`).
 */

import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

/**
 * Extract the single JSON value printed by the command, ignoring any
 * non-JSON log lines (e.g. environment-specific keyring warnings) that may be
 * interleaved on stdout by the tracing subscriber.
 */
// biome-ignore lint/suspicious/noExplicitAny: test file
function parseJsonOutput(stdout: string): any {
    for (const line of stdout.split(/\r?\n/)) {
        const trimmed = line.trim();
        if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
            try {
                return JSON.parse(trimmed);
            } catch {
                // keep scanning
            }
        }
    }
    throw new Error(`no JSON value found in stdout:\n${stdout}`);
}

function runtimeAvailable(bin: string): boolean {
    try {
        return spawnSync(bin, ["--version"], { stdio: "ignore" }).status === 0;
    } catch {
        return false;
    }
}

const GREET_TOOL = `type: js
name: greet
description: Greet a user a number of times
entrypoint: ./index.mjs
inputs:
  - type: string
    name: who
  - type: integer
    name: times
    default: 1
`;

const GREET_SCRIPT = `export default function greet(ctx) {
    const { who, times } = ctx.inputs;
    return { greeting: \`hello \${who}\`, times, timesKind: typeof times };
}
`;

function toolWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            tools: [{ source: "local", path: "tools/**" }],
        },
        files: {
            "tools/greet/tool.omni.yaml": GREET_TOOL,
            "tools/greet/index.mjs": GREET_SCRIPT,
        },
    };
}

describe.runIf(runtimeAvailable("deno"))(
    "+tool @e2e",
    { tags: ["tool"] },
    () => {
        it("lists discovered tools", async () => {
            const ws = makeWorkspace(toolWorkspace());
            const result = await runOmni(["tool", "list"], { cwd: ws.cwd });
            expect(result).toHaveSucceeded();
            expect(result.stdout).toContain("greet");
        });

        it("inspects a tool's input schema", async () => {
            const ws = makeWorkspace(toolWorkspace());
            const result = await runOmni(["tool", "inspect", "greet"], {
                cwd: ws.cwd,
            });
            expect(result).toHaveSucceeded();
            const schema = parseJsonOutput(result.stdout);
            expect(schema.properties).toHaveProperty("who");
            expect(schema.properties).toHaveProperty("times");
            expect(schema.required).toContain("who");
        });

        it("runs a js tool and captures its return value as JSON", async () => {
            const ws = makeWorkspace(toolWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "greet",
                    "--args-json",
                    JSON.stringify({ who: "world", times: 3 }),
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            const value = parseJsonOutput(result.stdout);
            expect(value).toEqual({
                greeting: "hello world",
                times: 3,
                timesKind: "number",
            });
        });

        it("merges --arg over --args-json with --arg winning", async () => {
            const ws = makeWorkspace(toolWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "greet",
                    "--args-json",
                    JSON.stringify({ who: "world" }),
                    "--arg",
                    "who=alice",
                    "--arg",
                    "times=5",
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            const value = parseJsonOutput(result.stdout);
            expect(value.greeting).toBe("hello alice");
            expect(value.times).toBe(5);
            expect(value.timesKind).toBe("number");
        });

        it("fails when a required input is missing", async () => {
            const ws = makeWorkspace(toolWorkspace());
            const result = await runOmni(
                ["tool", "run", "greet", "--arg", "times=2"],
                { cwd: ws.cwd },
            );
            expect(result.exitCode).not.toBe(0);
            expect(`${result.stdout}${result.stderr ?? ""}`).toContain("who");
        });
    },
);

// Capability enforcement for tools mirrors generators: it is gated behind the
// experimental `capabilities` switch, and a tool that declares nothing is held
// to the built-in floor (`@workspace/**` fs read+write). Filesystem access
// through `ctx.sys.fs` is brokered in-process, so these checks are
// deterministic under the default `deno` runtime.
const WRITER_TOOL = `type: js
name: writer
description: Write a string to a path via the enforced sys
entrypoint: ./index.mjs
inputs:
  - type: string
    name: path
`;

const WRITER_SCRIPT = `export default async function (ctx) {
    await ctx.sys.fs.writeStringToFile(ctx.inputs.path, "ok");
    return { wrote: ctx.inputs.path };
}
`;

function enforcedToolWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            tools: [{ source: "local", path: "tools/**" }],
            // Enforcement is gated behind the experimental switch; opt in.
            enable_experimental: { capabilities: true },
        },
        files: {
            "tools/writer/tool.omni.yaml": WRITER_TOOL,
            "tools/writer/index.mjs": WRITER_SCRIPT,
        },
    };
}

describe.runIf(runtimeAvailable("deno"))(
    "+tool @e2e (capabilities)",
    { tags: ["tool"], timeout: 60_000 },
    () => {
        it("permits a write within the built-in workspace floor", async () => {
            const ws = makeWorkspace(enforcedToolWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "writer",
                    "--args-json",
                    JSON.stringify({ path: "allowed.txt" }),
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            expect(ws.read("allowed.txt")).toBe("ok");
        });

        it("denies a write escaping the workspace floor", async () => {
            const ws = makeWorkspace(enforcedToolWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "writer",
                    "--args-json",
                    JSON.stringify({ path: "../escaped.txt" }),
                ],
                { cwd: ws.cwd },
            );
            expect(result.exitCode).not.toBe(0);
            expect(ws.exists("../escaped.txt")).toBe(false);
        });
    },
);

// A `type: pipeline` tool chains other tools, routing one step's output into the
// next step's inputs via `from:` references (JSON types preserved, no string
// round-trips). `tool inspect` reports the pipeline's own inputs, not its steps'.
const FETCH_TOOL = `type: js
name: fetch
description: Produce a fixed file list
entrypoint: ./index.mjs
inputs:
  - type: string
    name: dir
`;

const FETCH_SCRIPT = `export default function () {
    return { files: ["a.txt", "b.txt"], count: 2 };
}
`;

const SUMMARIZE_TOOL = `type: js
name: summarize
description: Summarize a file list
entrypoint: ./index.mjs
inputs:
  - type: string-array
    name: files
  - type: integer
    name: count
`;

const SUMMARIZE_SCRIPT = `export default function (ctx) {
    const { files, count } = ctx.inputs;
    return { summary: count + " files", files, countKind: typeof count };
}
`;

const REPORT_PIPELINE = `type: pipeline
name: report
description: Fetch then summarize
inputs:
  - type: string
    name: dir
steps:
  - name: fetch
    tool: fetch
    inputs:
      dir: { from: inputs.dir }
  - name: summarize
    tool: summarize
    inputs:
      files: { from: steps.fetch.output.files }
      count: { from: steps.fetch.output.count }
`;

function pipelineWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            tools: [{ source: "local", path: "tools/**" }],
        },
        files: {
            "tools/fetch/tool.omni.yaml": FETCH_TOOL,
            "tools/fetch/index.mjs": FETCH_SCRIPT,
            "tools/summarize/tool.omni.yaml": SUMMARIZE_TOOL,
            "tools/summarize/index.mjs": SUMMARIZE_SCRIPT,
            "tools/report/tool.omni.yaml": REPORT_PIPELINE,
        },
    };
}

describe.runIf(runtimeAvailable("deno"))(
    "+tool @e2e (pipeline)",
    { tags: ["tool"], timeout: 60_000 },
    () => {
        it("chains two js tools and returns the last step's output", async () => {
            const ws = makeWorkspace(pipelineWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "report",
                    "--args-json",
                    JSON.stringify({ dir: "/data" }),
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            const value = parseJsonOutput(result.stdout);
            // The `count` reference preserved its JSON number type across steps.
            expect(value).toEqual({
                summary: "2 files",
                files: ["a.txt", "b.txt"],
                countKind: "number",
            });
        });

        it("inspects the pipeline's own inputs, not its steps'", async () => {
            const ws = makeWorkspace(pipelineWorkspace());
            const result = await runOmni(["tool", "inspect", "report"], {
                cwd: ws.cwd,
            });
            expect(result).toHaveSucceeded();
            const schema = parseJsonOutput(result.stdout);
            expect(schema.properties).toHaveProperty("dir");
            expect(schema.properties).not.toHaveProperty("files");
            expect(schema.properties).not.toHaveProperty("count");
        });
    },
);

// The optional working directory a tool operates in: relative `ctx.sys` paths
// resolve against `--cwd <path>` or the directory of `--project <name>` (the two
// are mutually exclusive). Defaults to the workspace root.
function workingDirWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            tools: [{ source: "local", path: "tools/**" }],
            enable_experimental: { capabilities: true },
        },
        projects: {
            "pkg/project.omni.yaml": { name: "pkg" },
        },
        files: {
            "tools/writer/tool.omni.yaml": WRITER_TOOL,
            "tools/writer/index.mjs": WRITER_SCRIPT,
            "sub/.keep": "",
        },
    };
}

describe.runIf(runtimeAvailable("deno"))(
    "+tool @e2e (working dir)",
    { tags: ["tool"], timeout: 60_000 },
    () => {
        it("resolves relative writes against --cwd", async () => {
            const ws = makeWorkspace(workingDirWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "writer",
                    "--cwd",
                    "sub",
                    "--args-json",
                    JSON.stringify({ path: "out.txt" }),
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            expect(ws.read("sub/out.txt")).toBe("ok");
        });

        it("resolves relative writes against --project's directory", async () => {
            const ws = makeWorkspace(workingDirWorkspace());
            const result = await runOmni(
                [
                    "tool",
                    "run",
                    "writer",
                    "-p",
                    "pkg",
                    "--args-json",
                    JSON.stringify({ path: "out.txt" }),
                ],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            expect(ws.read("pkg/out.txt")).toBe("ok");
        });
    },
);

describe("+tool @e2e (working dir validation)", { tags: ["tool"] }, () => {
    it("rejects --cwd and --project together", async () => {
        const ws = makeWorkspace(workingDirWorkspace());
        const result = await runOmni(
            ["tool", "run", "writer", "--cwd", "sub", "-p", "pkg"],
            { cwd: ws.cwd },
        );
        expect(result.exitCode).not.toBe(0);
    });

    it("errors when --cwd does not exist", async () => {
        const ws = makeWorkspace(workingDirWorkspace());
        const result = await runOmni(
            [
                "tool",
                "run",
                "writer",
                "--cwd",
                "does-not-exist",
                "--args-json",
                JSON.stringify({ path: "out.txt" }),
            ],
            { cwd: ws.cwd },
        );
        expect(result.exitCode).not.toBe(0);
    });

    it("errors when --project is unknown", async () => {
        const ws = makeWorkspace(workingDirWorkspace());
        const result = await runOmni(
            [
                "tool",
                "run",
                "writer",
                "-p",
                "ghost",
                "--args-json",
                JSON.stringify({ path: "out.txt" }),
            ],
            { cwd: ws.cwd },
        );
        expect(result.exitCode).not.toBe(0);
    });
});
