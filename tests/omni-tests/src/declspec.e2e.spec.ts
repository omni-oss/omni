/**
 * `omni declspec <dump|schema>` - emits a machine-readable description of the
 * CLI (`dump`) or the JSON schema for that description (`schema`). Pinned to
 * `crates/omni_cli_core/src/commands/declspec.rs`.
 *
 * Caveat: like `completion`, `declspec` builds the full clap command tree; in
 * debug builds clap's debug assertions currently abort on a duplicate `-d` short
 * option in `cache prune`, so these rely on the (preferred) release binary.
 */

import { parse as parseToml } from "smol-toml";
import { describe, expect, it } from "vitest";
import { parse as parseYaml } from "yaml";
import { runOmni } from "@/harness";

const EXPECTED_SUBCOMMANDS = [
    "env",
    "exec",
    "config",
    "completion",
    "run",
    "hash",
    "declspec",
    "cache",
    "generator",
    "init",
    "project",
];

interface DeclspecDump {
    name: string;
    subcommands: Array<{ name: string }>;
    opts: Array<{ long?: string | null }>;
}

describe("+declspec @output (dump)", () => {
    it("emits the CLI spec as JSON by default", async () => {
        const result = await runOmni(["declspec", "dump"]);

        expect(result).toHaveSucceeded();
        const spec = JSON.parse(result.stdout) as DeclspecDump;
        expect(spec.name).toBe("omni");
        expect(Array.isArray(spec.subcommands)).toBe(true);
    });

    it("-f yaml emits the spec as YAML", async () => {
        const result = await runOmni(["declspec", "dump", "-f", "yaml"]);

        expect(result).toHaveSucceeded();
        const spec = parseYaml(result.stdout) as DeclspecDump;
        expect(spec.name).toBe("omni");
        expect(Array.isArray(spec.subcommands)).toBe(true);
    });

    it("-f toml emits the spec as TOML", async () => {
        const result = await runOmni(["declspec", "dump", "-f", "toml"]);

        expect(result).toHaveSucceeded();
        const spec = parseToml(result.stdout) as unknown as DeclspecDump;
        expect(spec.name).toBe("omni");
        expect(Array.isArray(spec.subcommands)).toBe(true);
    });

    it("enumerates every subcommand and the global flags", async () => {
        const result = await runOmni(["declspec", "dump"]);

        expect(result).toHaveSucceeded();
        const spec = JSON.parse(result.stdout) as DeclspecDump;

        const subcommands = spec.subcommands.map((sub) => sub.name);
        for (const name of EXPECTED_SUBCOMMANDS) {
            expect(subcommands).toContain(name);
        }

        // A couple of representative global options guard against spec drift.
        const longOpts = spec.opts.map((opt) => opt.long);
        expect(longOpts).toContain("stdout-logs-level");
        expect(longOpts).toContain("env-file");
    });
});

describe("+declspec @output (schema)", () => {
    it("emits the declspec JSON schema", async () => {
        const result = await runOmni(["declspec", "schema"]);

        expect(result).toHaveSucceeded();
        const schema = JSON.parse(result.stdout) as Record<string, unknown>;
        expect(schema.$schema).toMatch(/json-schema\.org/);
        expect(typeof schema.title).toBe("string");
    });

    it("-p pretty-prints the schema while keeping the same content", async () => {
        const compact = await runOmni(["declspec", "schema"]);
        const pretty = await runOmni(["declspec", "schema", "-p"]);

        expect(compact).toHaveSucceeded();
        expect(pretty).toHaveSucceeded();
        expect(compact.out).not.toContain("\n");
        expect(pretty.out).toContain("\n  ");
        expect(JSON.parse(pretty.stdout)).toEqual(JSON.parse(compact.stdout));
    });
});
