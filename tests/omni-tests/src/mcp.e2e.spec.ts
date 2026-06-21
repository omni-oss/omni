/**
 * `omni mcp` e2e tests: the MCP stdio server driven by the official
 * `@modelcontextprotocol/client` SDK.
 *
 * Each test creates a fresh temporary workspace, spawns `omni mcp --root-dir
 * <dir>` via {@link connectMcp}, exercises one or more tools, then lets the
 * harness disconnect automatically. Tests are deliberately isolated — no shared
 * state leaks between them via the process or workspace.
 */

import { normalize } from "node:path";
import type { Client } from "@modelcontextprotocol/client";
import { describe, expect, it } from "vitest";
import {
    connectMcp,
    makeWorkspace,
    runOmni,
    scaffoldGeneratorSpec,
    singleProjectSpec,
    type WorkspaceSpec,
} from "@/harness";
import { cleanPath } from "./utils";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** The return type of `client.callTool()`. */
type CallToolResult = Awaited<ReturnType<Client["callTool"]>>;

/**
 * Assert a tool result is not an error and return its structured content cast
 * to `T`. Tools that fail at the protocol level throw instead of populating
 * `isError`, so this only guards against the rare case where a tool returns
 * `is_error: true` in its payload.
 */
function getContent<T>(result: CallToolResult): T {
    if (result.isError) {
        throw new Error(
            `Tool returned an error result: ${JSON.stringify(result)}`,
        );
    }
    if (!result.structuredContent) {
        throw new Error(
            `Tool result has no structuredContent: ${JSON.stringify(result)}`,
        );
    }
    return result.structuredContent as unknown as T;
}

/**
 * A workspace with a generator that appends a Tera-rendered entry after a
 * sentinel line in a workspace-level file (`@root/registry.txt`). This is the
 * minimal fixture needed to exercise the read-modify-write race: without the
 * workspace lock, concurrent runs would each read the original file and the
 * last writer would overwrite the other's entry.
 */
function appendGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/appender/generator.omni.yaml": {
                name: "appender",
                inputs: [
                    {
                        type: "text",
                        name: "entry",
                        message: "What to append?",
                    },
                ],
                // @workspace resolves to the workspace root, so this file is
                // shared across every generator run in the workspace.
                targets: { registry: "@workspace/registry.txt" },
                actions: [
                    {
                        type: "append-content",
                        target: "registry",
                        entries: [
                            {
                                pattern: "# ENTRIES",
                                content: "{{ inputs.entry }}",
                            },
                        ],
                    },
                ],
            },
        },
        files: {
            // Pre-seed the target file; append-content requires it to exist.
            "registry.txt": "# ENTRIES\n",
            ".omni/sources/generator/.keep": "",
        },
    };
}

/** Two-project workspace for filter / multi-project assertions. */
function twoProjectSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            alpha: { name: "alpha", tasks: { build: 'echo "alpha build"' } },
            beta: { name: "beta", tasks: { build: 'echo "beta build"' } },
        },
    };
}

/**
 * A workspace whose generator has a conditional input gated on the value of a
 * preceding input. `version` is a `select` defaulting to `"custom"`, and
 * `crate_version` is only active when `version == 'custom'`.
 *
 * This mirrors the real `crate` generator and exercises default-value
 * propagation during validation: the `if` expression references
 * `inputs.version`, which is only present if the resolved default of `version`
 * is threaded into the condition context. Without that propagation the Tera
 * render of `{{ inputs.version == 'custom' }}` fails and validation reports a
 * `_configuration` error instead of evaluating the input tree.
 */
function conditionalGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/conditional/generator.omni.yaml": {
                name: "conditional",
                description: "conditional input gated on another input's value",
                inputs: [
                    {
                        type: "select",
                        name: "version",
                        message: "Version",
                        default: "custom",
                        options: [
                            {
                                name: "Inherit from workspace",
                                value: "workspace",
                            },
                            { name: "Custom", value: "custom" },
                        ],
                    },
                    {
                        if: "{{ inputs.version == 'custom' }}",
                        type: "text",
                        name: "crate_version",
                        message: "Crate version",
                        default: "0.0.0",
                    },
                ],
                actions: [
                    {
                        type: "add-content",
                        output_path: "version.txt",
                        content: "{{ inputs.version }}",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/**
 * Like {@link conditionalGeneratorSpec} but the conditional input has no
 * default, so it is *required* whenever its `if` condition is active. This lets
 * tests observe whether the condition evaluated truthy (input required) or
 * falsy (input skipped) purely from the validation result.
 */
function conditionalRequiredGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/conditional/generator.omni.yaml": {
                name: "conditional",
                description:
                    "required conditional input gated on another input",
                inputs: [
                    {
                        type: "select",
                        name: "version",
                        message: "Version",
                        default: "custom",
                        options: [
                            {
                                name: "Inherit from workspace",
                                value: "workspace",
                            },
                            { name: "Custom", value: "custom" },
                        ],
                    },
                    {
                        if: "{{ inputs.version == 'custom' }}",
                        type: "text",
                        name: "crate_version",
                        message: "Crate version",
                    },
                ],
                actions: [
                    {
                        type: "add-content",
                        output_path: "version.txt",
                        content: "{{ inputs.version }}",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/**
 * A generator that invokes itself through a `run-generator` action, forming a
 * direct recursion cycle (`loop → loop`). Running it must be rejected by the
 * up-front recursion check (`detect_recursion`) before any action executes.
 */
function selfRecursiveGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/loop/generator.omni.yaml": {
                name: "loop",
                description: "calls itself, forming a direct recursion cycle",
                inputs: [],
                actions: [{ type: "run-generator", generator: "loop" }],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/**
 * Two generators that invoke each other (`ping → pong → ping`), forming an
 * indirect recursion cycle. Running either entry point must be rejected.
 */
function mutualRecursionGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/ping/generator.omni.yaml": {
                name: "ping",
                description: "calls pong",
                inputs: [],
                actions: [{ type: "run-generator", generator: "pong" }],
            },
            "generators/pong/generator.omni.yaml": {
                name: "pong",
                description: "calls ping",
                inputs: [],
                actions: [{ type: "run-generator", generator: "ping" }],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/**
 * A legitimate, non-cyclic composition: `parent` runs `child`, which writes a
 * file. This is the positive control proving `detect_recursion` does not
 * false-positive on a valid `run-generator` chain and that nested generation
 * actually executes.
 */
function nestedGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/parent/generator.omni.yaml": {
                name: "parent",
                description: "runs the child generator",
                inputs: [],
                actions: [{ type: "run-generator", generator: "child" }],
            },
            "generators/child/generator.omni.yaml": {
                name: "child",
                description: "writes a nested file",
                inputs: [],
                actions: [
                    {
                        type: "add-content",
                        output_path: "nested.txt",
                        content: "from child",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

// ---------------------------------------------------------------------------
// Protocol — tools/list
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (protocol)", () => {
    it("tools/list returns all 13 expected tools", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const { tools } = await client.listTools();

        const names = tools.map((t) => t.name).sort();
        expect(names).toEqual(
            [
                "cache_prune",
                "cache_stats",
                "exec_command",
                "generator_inspect",
                "generator_list",
                "generator_run",
                "generator_validate_input",
                "hash_project",
                "hash_workspace",
                "project_config",
                "project_list",
                "run_tasks",
                "workspace_info",
            ].sort(),
        );
    });

    it("read-only tools carry readOnlyHint: true", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const { tools } = await client.listTools();
        const byName = Object.fromEntries(tools.map((t) => [t.name, t]));

        const readOnlyTools = [
            "workspace_info",
            "project_list",
            "project_config",
            "generator_list",
            "generator_inspect",
            "generator_validate_input",
            "hash_workspace",
            "hash_project",
            "cache_stats",
        ];
        for (const name of readOnlyTools) {
            expect(
                byName[name]?.annotations?.readOnlyHint,
                `${name} should be read-only`,
            ).toBe(true);
        }
    });

    it("write tools do not have readOnlyHint set", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const { tools } = await client.listTools();
        const byName = Object.fromEntries(tools.map((t) => [t.name, t]));

        const writeTools = [
            "generator_run",
            "cache_prune",
            "run_tasks",
            "exec_command",
        ];
        for (const name of writeTools) {
            expect(
                byName[name]?.annotations?.readOnlyHint,
                `${name} should not be read-only`,
            ).not.toBe(true);
        }
    });
});

// ---------------------------------------------------------------------------
// workspace_info
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (workspace_info)", () => {
    it("returns root_dir matching the workspace directory", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "workspace_info" });
        const data = getContent<{ root_dir: string; cache_dir: string }>(
            result,
        );

        expect(cleanPath(data.root_dir)).toBe(cleanPath(ws.cwd));
    });

    it("cache_dir is nested inside root_dir", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "workspace_info" });
        const data = getContent<{ root_dir: string; cache_dir: string }>(
            result,
        );

        expect(normalize(data.cache_dir)).toContain(normalize(data.root_dir));
    });

    it("env_vars is a string-to-string map", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "workspace_info" });
        const data = getContent<{
            env_vars: Record<string, unknown>;
        }>(result);

        expect(typeof data.env_vars).toBe("object");
        for (const value of Object.values(data.env_vars)) {
            expect(typeof value).toBe("string");
        }
    });
});

// ---------------------------------------------------------------------------
// project_list
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (project_list)", () => {
    it("lists every project in the workspace", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "project_list" });
        const data = getContent<{ projects: string[] }>(result);

        expect(data.projects).toContain("app");
    });

    it("lists all projects in a multi-project workspace", async () => {
        const ws = makeWorkspace(twoProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "project_list" });
        const data = getContent<{ projects: string[] }>(result);

        expect(data.projects).toContain("alpha");
        expect(data.projects).toContain("beta");
        expect(data.projects).toHaveLength(2);
    });
});

// ---------------------------------------------------------------------------
// project_config
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (project_config)", () => {
    it("returns config and tasks for a known project", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "project_config",
            arguments: { name: "app" },
        });
        const data = getContent<{
            name: string;
            dir: string;
            tasks: Array<{ name: string }>;
        }>(result);

        expect(data.name).toBe("app");
        expect(data.dir).toBeTruthy();
        const taskNames = data.tasks.map((t) => t.name);
        expect(taskNames).toContain("build");
        expect(taskNames).toContain("test");
    });

    it("rejects an unknown project with a protocol error", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "project_config",
                arguments: { name: "does-not-exist" },
            }),
        ).rejects.toThrow();
    });
});

// ---------------------------------------------------------------------------
// generator_list
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_list)", () => {
    it("lists generators declared in the workspace", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "generator_list" });
        const data = getContent<{
            generators: Array<{ name: string; description?: string }>;
        }>(result);

        const scaffold = data.generators.find((g) => g.name === "scaffold");
        expect(scaffold).toBeDefined();
        expect(scaffold?.description).toBe("scaffolds a greeting file");
    });

    it("returns an empty list when no generators are configured", async () => {
        // Strip the generators source from the scaffold spec but keep the
        // seeded `.omni/sources/generator/` directory — without it the
        // generator lock-file path doesn't exist and omni errors on Windows.
        const base = scaffoldGeneratorSpec();
        const ws = makeWorkspace({ ...base, workspace: { projects: ["**"] } });
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "generator_list" });
        const data = getContent<{ generators: unknown[] }>(result);

        expect(data.generators).toHaveLength(0);
    });

    it("omits generators with `user_invocable: false`", async () => {
        // A non-user-invocable generator is only callable by other generators,
        // so `generator_list` must not expose it to MCP clients.
        const spec = scaffoldGeneratorSpec();
        if (spec.projects) {
            spec.projects["generators/internal/generator.omni.yaml"] = {
                name: "internal-helper",
                description: "only callable by other generators",
                user_invocable: false,
                actions: [],
            };
        }
        const ws = makeWorkspace(spec);
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "generator_list" });
        const data = getContent<{
            generators: Array<{ name: string; description?: string }>;
        }>(result);

        const names = data.generators.map((g) => g.name);
        // The user-invocable generator is exposed...
        expect(names).toContain("scaffold");
        // ...while the non-user-invocable one is filtered out.
        expect(names).not.toContain("internal-helper");
    });
});

// ---------------------------------------------------------------------------
// generator_inspect
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_inspect)", () => {
    it("returns the input schema and targets for a generator", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_inspect",
            arguments: { name: "scaffold" },
        });
        const data = getContent<{
            name: string;
            description?: string;
            inputs: Array<{
                name: string;
                kind: string;
                required: boolean;
                default?: unknown;
                remember: boolean;
            }>;
            targets: Array<{ key: string; default_path: string }>;
        }>(result);

        expect(data.name).toBe("scaffold");
        expect(data.description).toBe("scaffolds a greeting file");

        const subject = data.inputs.find((i) => i.name === "subject");
        expect(subject).toBeDefined();
        expect(subject?.kind).toBe("text");
        // Has a static default "world" so it is not required.
        expect(subject?.required).toBe(false);
        expect(subject?.default).toBe("world");
        // Declared with remember: true in the fixture.
        expect(subject?.remember).toBe(true);

        expect(data.targets.find((t) => t.key === "dest")).toBeDefined();
    });

    it("rejects an unknown generator with a protocol error", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "generator_inspect",
                arguments: { name: "no-such-generator" },
            }),
        ).rejects.toThrow();
    });
});

// ---------------------------------------------------------------------------
// generator_run
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_run)", () => {
    it("dry_run=true reports actions without writing files", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_run",
            arguments: {
                name: "scaffold",
                output_dir: ws.path("out"),
                dry_run: true,
                use_defaults: true,
            },
        });
        const data = getContent<{ ok: boolean; actions: unknown[] }>(result);

        expect(data.ok).toBe(true);
        expect(data.actions.length).toBeGreaterThan(0);
        // File must NOT exist when dry_run is true.
        expect(ws.exists("out/src/greeting.txt")).toBe(false);
    });

    it("scaffolds files with use_defaults=true", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_run",
            arguments: {
                name: "scaffold",
                output_dir: ws.path("out"),
                dry_run: false,
                use_defaults: true,
                save_session: false,
                ignore_session: true,
            },
        });
        const data = getContent<{ ok: boolean }>(result);

        expect(data.ok).toBe(true);
        expect(ws.exists("out/src/greeting.txt")).toBe(true);
        expect(ws.read("out/src/greeting.txt")).toContain("Hello world!");
    });

    it("input_values override defaults in the generated output", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await client.callTool({
            name: "generator_run",
            arguments: {
                name: "scaffold",
                output_dir: ws.path("custom"),
                dry_run: false,
                use_defaults: false,
                save_session: false,
                ignore_session: true,
                input_values: { subject: "MCP" },
            },
        });

        expect(ws.read("custom/src/greeting.txt")).toContain("Hello MCP!");
    });
});

// ---------------------------------------------------------------------------
// generator_run — parallel / concurrency
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_run parallelism)", () => {
    it("concurrent runs to different output dirs all complete without errors", async () => {
        // Fire N runs simultaneously against separate output dirs. The workspace
        // lock serializes them internally, but since the outputs are disjoint
        // every run should succeed and write the correct file.
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const N = 5;
        const results = await Promise.all(
            Array.from({ length: N }, (_, i) =>
                client.callTool({
                    name: "generator_run",
                    arguments: {
                        name: "scaffold",
                        output_dir: ws.path(`out-${i}`),
                        dry_run: false,
                        use_defaults: false,
                        save_session: false,
                        ignore_session: true,
                        input_values: { subject: `Run${i}` },
                    },
                }),
            ),
        );

        results.forEach((result, i) => {
            const data = getContent<{ ok: boolean }>(result);
            expect(data.ok).toBe(true);
            expect(ws.exists(`out-${i}/src/greeting.txt`)).toBe(true);
            expect(ws.read(`out-${i}/src/greeting.txt`)).toContain(
                `Hello Run${i}!`,
            );
        });
    });

    it("concurrent runs modifying a shared workspace-level file preserve all changes", async () => {
        // This directly exercises the read-modify-write race that prompted the
        // workspace lock. The `append-content` action reads `registry.txt`,
        // inserts a line after the sentinel, and writes it back. Without
        // serialization the second concurrent run would read the original file
        // and overwrite the first run's entry. With the lock both entries survive.
        const ws = makeWorkspace(appendGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await Promise.all([
            client.callTool({
                name: "generator_run",
                arguments: {
                    name: "appender",
                    output_dir: ws.path("out"),
                    dry_run: false,
                    use_defaults: false,
                    save_session: false,
                    ignore_session: true,
                    input_values: { entry: "alpha" },
                },
            }),
            client.callTool({
                name: "generator_run",
                arguments: {
                    name: "appender",
                    output_dir: ws.path("out"),
                    dry_run: false,
                    use_defaults: false,
                    save_session: false,
                    ignore_session: true,
                    input_values: { entry: "beta" },
                },
            }),
        ]);

        // Both entries must appear — a race would cause one to be silently lost.
        const registry = ws.read("registry.txt");
        expect(registry).toContain("alpha");
        expect(registry).toContain("beta");
    });
});

// ---------------------------------------------------------------------------
// generator_run — recursion detection
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_run recursion)", () => {
    it("rejects a generator that directly invokes itself", async () => {
        // loop → loop. detect_recursion runs before any action executes, so the
        // call must reject rather than recurse until exhaustion.
        const ws = makeWorkspace(selfRecursiveGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "generator_run",
                arguments: {
                    name: "loop",
                    output_dir: ws.path("out"),
                    dry_run: false,
                    use_defaults: true,
                    save_session: false,
                    ignore_session: true,
                },
            }),
        ).rejects.toThrow();

        // Nothing should have been written.
        expect(ws.exists("out")).toBe(false);
    });

    it("rejects a generator caught in an indirect (mutual) recursion cycle", async () => {
        // ping → pong → ping.
        const ws = makeWorkspace(mutualRecursionGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "generator_run",
                arguments: {
                    name: "ping",
                    output_dir: ws.path("out"),
                    dry_run: false,
                    use_defaults: true,
                    save_session: false,
                    ignore_session: true,
                },
            }),
        ).rejects.toThrow();
    });

    it("rejects recursion even on a dry run", async () => {
        // The recursion guard precedes the dry-run/commit split, so a dry run is
        // rejected just like a real run.
        const ws = makeWorkspace(selfRecursiveGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "generator_run",
                arguments: {
                    name: "loop",
                    output_dir: ws.path("out"),
                    dry_run: true,
                    use_defaults: true,
                    save_session: false,
                    ignore_session: true,
                },
            }),
        ).rejects.toThrow();
    });

    it("runs a valid non-cyclic run-generator chain to completion", async () => {
        // parent → child (no cycle). Proves detect_recursion does not
        // false-positive on legitimate composition and that the nested
        // generator actually executes and writes its file.
        const ws = makeWorkspace(nestedGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_run",
            arguments: {
                name: "parent",
                output_dir: ws.path("out"),
                dry_run: false,
                use_defaults: true,
                save_session: false,
                ignore_session: true,
            },
        });
        const data = getContent<{ ok: boolean }>(result);

        expect(data.ok).toBe(true);
        expect(ws.exists("out/nested.txt")).toBe(true);
        expect(ws.read("out/nested.txt")).toContain("from child");
    });
});

// ---------------------------------------------------------------------------
// generator_run — max_depth (configurable nesting limit)
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_run max_depth)", () => {
    it("aborts a legitimate (acyclic) chain when max_depth is set below its nesting", async () => {
        // parent → child is a valid, non-cyclic chain: child runs at depth 1.
        // With max_depth=0 the nested run exceeds the cap and is rejected even
        // though there is no recursion. Proves the depth cap is not
        // recursion-specific and is honored downward.
        const ws = makeWorkspace(nestedGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "generator_run",
                arguments: {
                    name: "parent",
                    output_dir: ws.path("out"),
                    dry_run: false,
                    use_defaults: true,
                    save_session: false,
                    ignore_session: true,
                    max_depth: 0,
                },
            }),
        ).rejects.toThrow();

        // The nested write must not have happened.
        expect(ws.exists("out/nested.txt")).toBe(false);
    });

    it("runs the same chain when max_depth is raised to allow the nesting", async () => {
        // Same fixture, but an explicit, generous max_depth lets the depth-1
        // nested generator run. Proves the cap is configurable upward.
        const ws = makeWorkspace(nestedGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_run",
            arguments: {
                name: "parent",
                output_dir: ws.path("out"),
                dry_run: false,
                use_defaults: true,
                save_session: false,
                ignore_session: true,
                max_depth: 5,
            },
        });
        const data = getContent<{ ok: boolean }>(result);

        expect(data.ok).toBe(true);
        expect(ws.read("out/nested.txt")).toContain("from child");
    });
});

// ---------------------------------------------------------------------------
// generator_validate_input
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (generator_validate_input)", () => {
    it("valid=true when an explicit value is provided for the required input", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_validate_input",
            arguments: {
                name: "scaffold",
                input_values: { subject: "Alice" },
                use_defaults: false,
            },
        });
        const data = getContent<{ valid: boolean; errors: unknown[] }>(result);

        expect(data.valid).toBe(true);
        expect(data.errors).toHaveLength(0);
    });

    it("valid=true when use_defaults=true fills inputs without explicit values", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_validate_input",
            arguments: {
                name: "scaffold",
                input_values: {},
                use_defaults: true,
            },
        });
        const data = getContent<{ valid: boolean }>(result);

        expect(data.valid).toBe(true);
    });

    it("valid=true with use_defaults=true when a conditional input is gated on another input's default", async () => {
        // Regression: the `crate_version` input's `if` references
        // `inputs.version`, which is only resolved via `version`'s default.
        // Before default-value propagation in validation, rendering
        // `{{ inputs.version == 'custom' }}` failed and surfaced as a
        // `_configuration` error rather than a clean valid result.
        const ws = makeWorkspace(conditionalGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_validate_input",
            arguments: {
                name: "conditional",
                input_values: {},
                use_defaults: true,
            },
        });
        const data = getContent<{
            valid: boolean;
            errors: Array<{ input_name: string; message: string }>;
        }>(result);

        expect(data.valid).toBe(true);
        expect(data.errors).toHaveLength(0);
    });

    it("evaluates a conditional input's gate against an explicitly provided parent value", async () => {
        // version=workspace makes the `{{ inputs.version == 'custom' }}` gate
        // false, so `crate_version` is skipped entirely and is not required —
        // even with use_defaults=false.
        const ws = makeWorkspace(conditionalRequiredGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_validate_input",
            arguments: {
                name: "conditional",
                input_values: { version: "workspace" },
                use_defaults: false,
            },
        });
        const data = getContent<{ valid: boolean; errors: unknown[] }>(result);

        expect(data.valid).toBe(true);
        expect(data.errors).toHaveLength(0);
    });

    it("flags a required conditional input as missing when its gate is active", async () => {
        // version=custom activates the gate, so the required `crate_version`
        // (no default) must be reported missing rather than silently passing or
        // erroring on condition rendering.
        const ws = makeWorkspace(conditionalRequiredGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_validate_input",
            arguments: {
                name: "conditional",
                input_values: { version: "custom" },
                use_defaults: false,
            },
        });
        const data = getContent<{
            valid: boolean;
            errors: Array<{ input_name: string; message: string }>;
        }>(result);

        expect(data.valid).toBe(false);
        expect(data.errors.some((e) => e.input_name === "crate_version")).toBe(
            true,
        );
        // Must be a per-field error, not a config-level rendering failure.
        expect(data.errors.some((e) => e.input_name === "_configuration")).toBe(
            false,
        );
    });

    it("required conditional input passes when its gate is active via the parent default", async () => {
        // No version provided → version resolves to its default "custom" →
        // gate active → crate_version is required and is satisfied here by an
        // explicit value, proving the default propagates into the gate.
        const ws = makeWorkspace(conditionalRequiredGeneratorSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "generator_validate_input",
            arguments: {
                name: "conditional",
                input_values: { crate_version: "1.2.3" },
                use_defaults: true,
            },
        });
        const data = getContent<{ valid: boolean; errors: unknown[] }>(result);

        expect(data.valid).toBe(true);
        expect(data.errors).toHaveLength(0);
    });
});

// ---------------------------------------------------------------------------
// hash_workspace
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (hash_workspace)", () => {
    it("returns a non-empty hash string", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({ name: "hash_workspace" });
        const data = getContent<{ hash: string }>(result);

        expect(typeof data.hash).toBe("string");
        expect(data.hash.length).toBeGreaterThan(0);
    });

    it("hash changes after a project config is modified", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const first = await client.callTool({ name: "hash_workspace" });
        const h1 = getContent<{ hash: string }>(first).hash;

        ws.write("app/project.omni.yaml", {
            name: "app",
            tasks: {
                build: 'echo "modified build"',
                test: 'echo "test app"',
            },
        });

        const second = await client.callTool({ name: "hash_workspace" });
        const h2 = getContent<{ hash: string }>(second).hash;

        expect(h1).not.toBe(h2);
    });
});

// ---------------------------------------------------------------------------
// hash_project
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (hash_project)", () => {
    it("returns a hash for a named project", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "hash_project",
            arguments: { name: "app" },
        });
        const data = getContent<{ hash: string }>(result);

        expect(typeof data.hash).toBe("string");
        expect(data.hash.length).toBeGreaterThan(0);
    });

    it("project hash differs from workspace hash in a multi-project workspace", async () => {
        const ws = makeWorkspace(twoProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const wsResult = await client.callTool({ name: "hash_workspace" });
        const projResult = await client.callTool({
            name: "hash_project",
            arguments: { name: "alpha" },
        });

        const wsHash = getContent<{ hash: string }>(wsResult).hash;
        const projHash = getContent<{ hash: string }>(projResult).hash;

        expect(wsHash).not.toBe(projHash);
    });

    it("rejects an unknown project with a protocol error", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        await expect(
            client.callTool({
                name: "hash_project",
                arguments: { name: "does-not-exist" },
            }),
        ).rejects.toThrow();
    });
});

// ---------------------------------------------------------------------------
// cache_stats
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (cache_stats)", () => {
    it("returns an empty project list for a fresh workspace", async () => {
        // Seed .omni/cache/ so the cache directory chain exists on Windows;
        // without it omni errors when trying to read from a non-existent path.
        const ws = makeWorkspace({
            ...singleProjectSpec(),
            files: { ".omni/cache/.keep": "" },
        });
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "cache_stats",
            arguments: {},
        });
        const data = getContent<{ projects: unknown[] }>(result);

        expect(data.projects).toHaveLength(0);
    });

    it("reflects entries after tasks have been run", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const seed = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(seed).toHaveSucceeded();

        const { client } = await connectMcp({ cwd: ws.cwd });
        const result = await client.callTool({
            name: "cache_stats",
            arguments: {},
        });
        const data = getContent<{
            projects: Array<{
                project_name: string;
                tasks: Array<{ task_name: string }>;
            }>;
        }>(result);

        const appProject = data.projects.find((p) => p.project_name === "app");
        expect(appProject).toBeDefined();
        expect(
            appProject?.tasks.find((t) => t.task_name === "build"),
        ).toBeDefined();
    });

    it("project filter narrows results to matching projects", async () => {
        const ws = makeWorkspace(twoProjectSpec());
        const seed = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(seed).toHaveSucceeded();

        const { client } = await connectMcp({ cwd: ws.cwd });
        const result = await client.callTool({
            name: "cache_stats",
            arguments: { project: ["alpha"] },
        });
        const data = getContent<{
            projects: Array<{ project_name: string }>;
        }>(result);

        expect(data.projects.every((p) => p.project_name === "alpha")).toBe(
            true,
        );
    });
});

// ---------------------------------------------------------------------------
// cache_prune
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (cache_prune)", () => {
    it("dry_run=true (default) reports entries without removing them", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const seed = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(seed).toHaveSucceeded();

        const { client } = await connectMcp({ cwd: ws.cwd });
        const pruneResult = await client.callTool({
            name: "cache_prune",
            arguments: { dry_run: true },
        });
        const pruneData = getContent<{
            dry_run: boolean;
            entries_pruned: number;
        }>(pruneResult);

        expect(pruneData.dry_run).toBe(true);

        // Cache must still be intact after a dry run.
        const statsResult = await client.callTool({
            name: "cache_stats",
            arguments: {},
        });
        const statsData = getContent<{ projects: unknown[] }>(statsResult);
        expect(statsData.projects.length).toBeGreaterThan(0);
    });

    it("dry_run=false reports that entries were pruned", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const seed = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(seed).toHaveSucceeded();

        const { client } = await connectMcp({ cwd: ws.cwd });
        const pruneResult = await client.callTool({
            name: "cache_prune",
            arguments: { dry_run: false },
        });
        const pruneData = getContent<{
            dry_run: boolean;
            entries_pruned: number;
            bytes_freed: number;
        }>(pruneResult);

        expect(pruneData.dry_run).toBe(false);
        expect(pruneData.entries_pruned).toBeGreaterThan(0);
        expect(typeof pruneData.bytes_freed).toBe("number");
    });
});

// ---------------------------------------------------------------------------
// run_tasks
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (run_tasks)", () => {
    it("runs a named task and reports completed status with exit code 0", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "run_tasks",
            arguments: { tasks: ["build"] },
        });
        const data = getContent<{
            ok: boolean;
            results: Array<{
                project: string;
                task: string;
                status: string;
                exit_code?: number;
            }>;
        }>(result);

        expect(data.ok).toBe(true);
        const buildResult = data.results.find((r) => r.task === "build");
        expect(buildResult).toBeDefined();
        expect(buildResult?.status).toBe("completed");
        expect(buildResult?.exit_code).toBe(0);
    });

    it("project filter limits execution to matching projects", async () => {
        const ws = makeWorkspace(twoProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "run_tasks",
            arguments: { tasks: ["build"], project: ["alpha"] },
        });
        const data = getContent<{
            ok: boolean;
            results: Array<{ project: string }>;
        }>(result);

        expect(data.ok).toBe(true);
        expect(data.results.every((r) => r.project === "alpha")).toBe(true);
    });
});

// ---------------------------------------------------------------------------
// exec_command
// ---------------------------------------------------------------------------

describe("+mcp @mcp @cli (exec_command)", () => {
    it("runs a command across all workspace projects and returns results", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "exec_command",
            arguments: { cmd: ["echo", "hello"] },
        });
        const data = getContent<{
            ok: boolean;
            results: Array<{ project: string; status: string }>;
        }>(result);

        // The result must be a valid object with the expected shape; `ok` may
        // be false on platforms where `echo` is a shell builtin rather than a
        // standalone binary.
        expect(typeof data.ok).toBe("boolean");
        expect(data.results.length).toBeGreaterThan(0);
    });

    it("project filter limits which projects receive the command", async () => {
        const ws = makeWorkspace(twoProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });

        const result = await client.callTool({
            name: "exec_command",
            arguments: { cmd: ["echo", "hello"], project: ["alpha"] },
        });
        const data = getContent<{
            ok: boolean;
            results: Array<{ project: string }>;
        }>(result);

        expect(data.results.every((r) => r.project === "alpha")).toBe(true);
    });
});
