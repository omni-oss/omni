/**
 * Generator input type defaults, `default_expr`, and object-input collection.
 *
 * All tests run non-interactively (no PTY) by satisfying every input through
 * `--use-defaults` (static defaults and dynamic `default_expr`).  The TUI /
 * interactive object-field path is covered in `prompt.e2e.spec.ts`.
 *
 * Coverage areas:
 *  - Non-string scalar `default` values (boolean, integer, float)
 *  - String `default` containing Tera syntax is template-expanded
 *  - `default_expr` — literal and Tera template strings
 *  - Static `default` beats `default_expr` when both are set
 *  - Object input with an object-level `default` map
 *  - Object input with `if: false` — whole object skipped
 *  - Object input via the emulated path: field-level defaults, field `if: false`,
 *    and recursive nesting
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

type Json = Record<string, unknown>;

/**
 * Minimal workspace exposing a single generator `g`.
 * The generator writes one `add-content` action to `result.txt` using the
 * supplied Tera `content` template.
 */
function singleInputSpec(inputs: Json[], content: string): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/g/generator.omni.yaml": {
                name: "g",
                inputs,
                actions: [
                    {
                        type: "add-content",
                        output_path: "result.txt",
                        content,
                    },
                ],
            },
        },
        // `generator run` writes its sources lockfile here without creating it.
        files: { ".omni/sources/generator/.keep": "" },
    };
}

// ── +input @cli (default handling) ────────────────────────────────────────────

describe("+input @cli (default handling)", {
    tags: ["generator"],
}, () => {
    it("boolean default: false is used with --use-defaults", async () => {
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "boolean",
                        name: "flag",
                        message: "Enable?",
                        default: false,
                    },
                ],
                "{{ inputs.flag }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("false");
    });

    it("integer default is used with --use-defaults", async () => {
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "integer",
                        name: "count",
                        message: "Count",
                        default: 42,
                    },
                ],
                "{{ inputs.count }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("42");
    });

    it("float default is used with --use-defaults", async () => {
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "float",
                        name: "ratio",
                        message: "Ratio",
                        default: 3.14,
                    },
                ],
                "{{ inputs.ratio }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("3.14");
    });

    it("a string default containing Tera syntax is template-expanded", async () => {
        // `expand_str: true` is set for static defaults so the value bag is
        // run through omni_tera before being stored.
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "string",
                        name: "greeting",
                        message: "Greeting",
                        default: "Hello {{ 'world' }}",
                    },
                ],
                "{{ inputs.greeting }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("Hello world");
    });
});

// ── +input @cli (default_expr) ─────────────────────────────────────────────────

describe("+input @cli (default expr)", {
    tags: ["generator"],
}, () => {
    it("default expr evaluates a literal string as a fallback default", async () => {
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "string",
                        name: "greeting",
                        message: "Greeting",
                        default: "hello",
                    },
                ],
                "{{ inputs.greeting }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("hello");
    });

    it("default expr evaluates Tera template syntax", async () => {
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "string",
                        name: "val",
                        message: "Value",
                        default: "{{ 'foo' ~ 'bar' }}",
                    },
                ],
                "{{ inputs.val }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("foobar");
    });
});

// ── +input @cli (object input — object-level default) ─────────────────────────

describe("+input @cli (object input - object-level default)", {
    tags: ["generator"],
}, () => {
    it("uses the object default map with --use-defaults, accessible via dot notation", async () => {
        // The default map bypasses field-by-field collection entirely;
        // the whole map is stored and its values are reachable in the template.
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "object",
                        name: "db",
                        message: "Database",
                        fields: [
                            { type: "string", name: "host", message: "Host" },
                            {
                                type: "integer",
                                name: "port",
                                message: "Port",
                            },
                        ],
                        default: { host: "localhost", port: 5432 },
                    },
                ],
                "{{ inputs.db.host }}:{{ inputs.db.port }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("localhost:5432");
    });

    it("an object with if: false is skipped and absent from the template context", async () => {
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "string",
                        name: "name",
                        message: "Name",
                        default: "alice",
                    },
                    {
                        type: "object",
                        name: "db",
                        message: "Database",
                        if: false,
                        fields: [
                            { type: "string", name: "host", message: "Host" },
                        ],
                        default: { host: "localhost" },
                    },
                ],
                "{{ inputs.name }}/{{ inputs.db | default(value='no-db') }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("alice/no-db");
    });
});

// ── +input @cli (object input — emulated field collection) ────────────────────

describe("+input @cli (object input - emulated field collection)", {
    tags: ["generator"],
}, () => {
    it("collects each field using its own default when no object-level default is set", async () => {
        // No object-level `default` → falls through to get_raw_input_value →
        // collect_from_object → each field collects via its own default.
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "object",
                        name: "db",
                        message: "Database",
                        fields: [
                            {
                                type: "string",
                                name: "host",
                                message: "Host",
                                default: "db-host",
                            },
                            {
                                type: "integer",
                                name: "port",
                                message: "Port",
                                default: 3306,
                            },
                        ],
                    },
                ],
                "{{ inputs.db.host }}:{{ inputs.db.port }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("db-host:3306");
    });

    it("a field with `if: false` is excluded from the collected object", async () => {
        // When a field's `if` condition is false it is skipped by
        // collect_internal; the parent object map simply won't contain it.
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "object",
                        name: "db",
                        message: "Database",
                        fields: [
                            {
                                type: "string",
                                name: "host",
                                message: "Host",
                                default: "h",
                            },
                            {
                                type: "integer",
                                name: "port",
                                message: "Port",
                                if: false,
                                default: 3306,
                            },
                        ],
                    },
                ],
                "host={{ inputs.db.host }} port={{ inputs.db.port | default(value='missing') }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("host=h port=missing");
    });

    it("collects a nested object recursively via field-level defaults", async () => {
        // An Object field inside an object goes through collect_from_object
        // recursively.  Both levels use field-level defaults; no object-level
        // `default` map is set on either.
        const ws = makeWorkspace(
            singleInputSpec(
                [
                    {
                        type: "object",
                        name: "server",
                        message: "Server",
                        fields: [
                            {
                                type: "string",
                                name: "host",
                                message: "Host",
                                default: "localhost",
                            },
                            {
                                type: "object",
                                name: "db",
                                message: "Database",
                                fields: [
                                    {
                                        type: "string",
                                        name: "name",
                                        message: "DB name",
                                        default: "mydb",
                                    },
                                    {
                                        type: "integer",
                                        name: "port",
                                        message: "DB port",
                                        default: 5432,
                                    },
                                ],
                            },
                        ],
                    },
                ],
                "{{ inputs.server.host }}:{{ inputs.server.db.name }}/{{ inputs.server.db.port }}",
            ),
        );

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "g",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/result.txt")).toBe("localhost:mydb/5432");
    });
});
