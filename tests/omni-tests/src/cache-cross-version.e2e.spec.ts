/**
 * Local cache entries written by a baseline `omni` binary (the pre-R08
 * build) must be read by the binary under test, and the reverse. Set
 * `OMNI_TEST_BASELINE_BIN` to run; skipped otherwise.
 */

import { execa } from "execa";
import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const baseline = process.env.OMNI_TEST_BASELINE_BIN;

function spec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                cache: { key: { defaults: false, files: ["src/**"] } },
                tasks: { build: 'echo "built"' },
            },
        },
        files: { "app/src/a.txt": "a\n" },
    };
}

const HIT = "Cache hit for task 'app#build'";

async function runBaseline(cwd: string) {
    const r = await execa(baseline!, ["run", "build"], {
        cwd,
        reject: false,
        timeout: 60_000,
    });
    return { code: r.exitCode, stderr: r.stderr };
}

describe.skipIf(!baseline)("+cache @cache @e2e (cross-version entries)", () => {
    it("reads an entry written by the baseline binary", async () => {
        const ws = makeWorkspace(spec());

        const first = await runBaseline(ws.cwd);
        expect(first.code).toBe(0);
        expect(first.stderr).not.toContain(HIT);

        const hit = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(hit).toHaveSucceeded();
        expect(hit).toHaveStderrContaining(HIT);
    });

    it("lets the baseline binary read an entry written by this binary", async () => {
        const ws = makeWorkspace(spec());

        expect(
            await runOmni(["run", "build"], { cwd: ws.cwd }),
        ).toHaveSucceeded();

        const hit = await runBaseline(ws.cwd);
        expect(hit.code).toBe(0);
        expect(hit.stderr).toContain(HIT);
    });
});
