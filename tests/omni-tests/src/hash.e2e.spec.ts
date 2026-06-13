/**
 * `omni hash <workspace|project>` - prints content hashes used for caching.
 * Pinned to `crates/omni_cli_core/src/commands/hash.rs`.
 *
 * Note: `-r/--raw` is a flag on the `hash` command (before the subcommand),
 * not on `hash project`. Non-raw runs also emit an "INFO Loaded context" log on
 * stdout, so we match the hash with a regex; raw runs suppress all logs and the
 * trailing newline, so we anchor the assertion.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, singleProjectSpec } from "@/harness";

// Hashes are long base58-ish tokens; the "Loaded context" log has no such run.
const HASH_PATTERN = /[A-Za-z0-9]{40,}/;
const RAW_HASH_PATTERN = /^[A-Za-z0-9]{40,}$/;

describe("+hash @e2e (workspace & project hashing)", () => {
    it("`omni hash workspace` prints a stable workspace hash", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["hash", "workspace"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toMatchOutput(HASH_PATTERN);
    });

    it("`omni hash project <name>` prints the project hash", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["hash", "project", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toMatchOutput(HASH_PATTERN);
    });

    it("-r/--raw prints only the hash, with no newline or log output", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["hash", "-r", "workspace"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.stdout).toMatch(RAW_HASH_PATTERN);
        expect(result.stdout).not.toContain("\n");
        expect(result.stdout).not.toContain("INFO");
    });

    it("`-t <task>` hashes the specified task", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const build = await runOmni(
            ["hash", "-r", "project", "app", "-t", "build"],
            { cwd: ws.cwd },
        );
        const test = await runOmni(
            ["hash", "-r", "project", "app", "-t", "test"],
            { cwd: ws.cwd },
        );

        expect(build).toHaveSucceeded();
        expect(test).toHaveSucceeded();
        expect(build.stdout).toMatch(RAW_HASH_PATTERN);
        expect(test.stdout).toMatch(RAW_HASH_PATTERN);
        // Different tasks must hash to different values.
        expect(build.stdout).not.toBe(test.stdout);
    });

    it("is deterministic across repeated runs with unchanged inputs", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const first = await runOmni(["hash", "-r", "workspace"], {
            cwd: ws.cwd,
        });
        const second = await runOmni(["hash", "-r", "workspace"], {
            cwd: ws.cwd,
        });

        expect(first).toHaveSucceeded();
        expect(first.stdout).toBe(second.stdout);
    });

    it("changes when a task command changes", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const before = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });

        ws.write("app/project.omni.yaml", {
            name: "app",
            tasks: { build: 'echo "changed"', test: 'echo "test app"' },
        });

        const after = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });

        expect(before).toHaveSucceeded();
        expect(after).toHaveSucceeded();
        expect(after.stdout).not.toBe(before.stdout);
    });

    it("errors clearly for a missing project", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["hash", "project", "does-not-exist"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no project found");
    });
});
