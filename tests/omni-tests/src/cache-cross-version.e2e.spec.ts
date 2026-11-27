/**
 * Mixed-version rehearsal for local cache entries: a baseline `omni` binary
 * (an earlier build, usually the base commit of the change) and the binary
 * under test run against one workspace and one local store. Skipped unless
 * `OMNI_TEST_BASELINE_BIN` names the baseline binary.
 *
 * Two modes, chosen by the relationship between the two binaries:
 *
 * - default: the cache key did not change between them, so each binary hits
 *   the entry the other wrote.
 * - `OMNI_TEST_EXPECT_COLD_START=1`: the baseline predates the last cache key
 *   change (a cold-start epoch, see `docs/patterns/cache-keys.md`), so neither
 *   binary hits the other's entry. Unset it for the first release whose
 *   baseline is after the key change.
 *
 * In both modes each binary hits its OWN entry, and an entry survives the
 * other binary running (the rollback case).
 */

import { execa } from "execa";
import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const baseline = process.env.OMNI_TEST_BASELINE_BIN;
const coldStart = process.env.OMNI_TEST_EXPECT_COLD_START === "1";

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

interface Side {
    name: string;
    run(cwd: string): Promise<{ code: number | undefined; stderr: string }>;
}

const baselineSide: Side = {
    name: "baseline",
    async run(cwd) {
        const r = await execa(baseline!, ["run", "build"], {
            cwd,
            reject: false,
            timeout: 60_000,
        });
        return { code: r.exitCode, stderr: r.stderr };
    },
};

const currentSide: Side = {
    name: "current",
    async run(cwd) {
        const r = await runOmni(["run", "build"], { cwd });
        return { code: r.exitCode, stderr: r.stderr };
    },
};

async function expectMiss(side: Side, cwd: string, why: string) {
    const r = await side.run(cwd);
    expect(r.code, `${side.name} run: ${why}`).toBe(0);
    expect(r.stderr, `${side.name} run: ${why}`).not.toContain(HIT);
}

async function expectHit(side: Side, cwd: string, why: string) {
    const r = await side.run(cwd);
    expect(r.code, `${side.name} run: ${why}`).toBe(0);
    expect(r.stderr, `${side.name} run: ${why}`).toContain(HIT);
}

async function rehearse(writer: Side, reader: Side) {
    const ws = makeWorkspace(spec());

    await expectMiss(writer, ws.cwd, "first run fills the store");
    await expectHit(writer, ws.cwd, "hits its own entry");

    if (coldStart) {
        await expectMiss(reader, ws.cwd, "cold start: not the other's entry");
    } else {
        await expectHit(reader, ws.cwd, "shares the other's entry");
    }
    await expectHit(reader, ws.cwd, "hits its own entry");

    // The reader running must not have disturbed the writer's entry.
    await expectHit(writer, ws.cwd, "still hits its own entry");
}

describe.skipIf(!baseline)(
    `+cache @cache @e2e (cross-version entries, ${coldStart ? "cold start" : "shared"})`,
    () => {
        it("the binary under test against an entry written by the baseline", async () => {
            await expect(
                rehearse(baselineSide, currentSide),
            ).resolves.toBeUndefined();
        });

        it("the baseline against an entry written by the binary under test", async () => {
            await expect(
                rehearse(currentSide, baselineSide),
            ).resolves.toBeUndefined();
        });
    },
);
