/**
 * `omni config schema <workspace|project|generator>` - prints the JSON Schema
 * for each configuration kind. Pinned to
 * `crates/omni_cli_core/src/commands/config.rs`.
 *
 * `config` takes no Context, so these run without a workspace.
 */

import { describe, expect, it } from "vitest";
import { runOmni } from "@/harness";

const SCHEMAS = ["workspace", "project", "generator"] as const;

/** Parse a command's stdout as JSON, failing the test with context on error. */
function parseSchema(stdout: string): Record<string, unknown> {
    try {
        return JSON.parse(stdout) as Record<string, unknown>;
    } catch (error) {
        throw new Error(
            `stdout was not valid JSON: ${String(error)}\n${stdout}`,
        );
    }
}

describe("+config @output (schema emission)", () => {
    for (const schema of SCHEMAS) {
        it(`\`omni config schema ${schema}\` emits a valid JSON schema`, async () => {
            const result = await runOmni(["config", "schema", schema]);

            expect(result).toHaveSucceeded();
            const parsed = parseSchema(result.stdout);
            expect(parsed.$schema).toMatch(/json-schema\.org/);
            expect(typeof parsed.title).toBe("string");
        });
    }

    it("-p/--pretty pretty-prints the schema while keeping the same content", async () => {
        const compact = await runOmni(["config", "schema", "workspace"]);
        const pretty = await runOmni([
            "config",
            "schema",
            "workspace",
            "--pretty",
        ]);

        expect(compact).toHaveSucceeded();
        expect(pretty).toHaveSucceeded();

        // Compact output is a single JSON line; pretty output is indented.
        expect(compact.out).not.toContain("\n");
        expect(pretty.out).toContain("\n  ");

        // Both must represent the exact same schema.
        expect(parseSchema(pretty.stdout)).toEqual(parseSchema(compact.stdout));
    });

    it("rejects an invalid schema name with value-enum help", async () => {
        const result = await runOmni(["config", "schema", "bogus"]);

        expect(result).toHaveExitCode(2);
        expect(result).toHaveStderrContaining("invalid value 'bogus'");
        expect(result).toHaveStderrContaining(
            "[possible values: workspace, project, generator]",
        );
    });

    it("-p pretty output differs from compact only by whitespace", async () => {
        // Use the short `-p` flag and a different schema than the long-form
        // test above to confirm both spellings pretty-print equivalently.
        const compact = await runOmni(["config", "schema", "project"]);
        const pretty = await runOmni(["config", "schema", "project", "-p"]);

        expect(compact).toHaveSucceeded();
        expect(pretty).toHaveSucceeded();

        // Compact output is a single line; pretty output gains newlines and
        // indentation that the compact form lacks.
        expect(compact.out).not.toContain("\n");
        expect(pretty.out).toContain("\n");
        expect(pretty.out).toContain("\n  ");

        // Stripping all whitespace from each form yields identical text, and
        // the parsed objects are deeply equal - the only difference is layout.
        const stripWs = (s: string) => s.replace(/\s+/g, "");
        expect(stripWs(pretty.stdout)).toBe(stripWs(compact.stdout));
        expect(parseSchema(pretty.stdout)).toEqual(parseSchema(compact.stdout));
    });

    it("all three schemas expose distinct top-level titles", async () => {
        const titles: string[] = [];
        for (const schema of SCHEMAS) {
            const result = await runOmni(["config", "schema", schema]);
            expect(result).toHaveSucceeded();
            const parsed = parseSchema(result.stdout);
            expect(typeof parsed.title).toBe("string");
            titles.push(parsed.title as string);
        }

        // Each configuration kind names itself uniquely.
        expect(new Set(titles).size).toBe(SCHEMAS.length);
    });
});
