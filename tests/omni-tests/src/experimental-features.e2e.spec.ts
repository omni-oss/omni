/**
 * End-to-end coverage for the workspace `enable_experimental` switch.
 *
 * Capability-based sandboxing is gated behind this switch. This suite proves
 * the gate itself (independent of the capability matrix in
 * `generator-capabilities.e2e.spec.ts`):
 *
 *   - off (the default): the declared capability policy is ignored at
 *     enforcement time and the generator script runs unconfined, so a write
 *     outside the declared allow-list is committed;
 *   - on: the policy is enforced and the same write is denied.
 *
 * Both accepted config shapes are exercised — the bare boolean form
 * (`enable_experimental: true`) and the per-feature map form
 * (`enable_experimental: { capabilities: true }`).
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

type Capability = {
    access: "allow" | "deny";
    domain: "fs.read" | "fs.write" | "net" | "process" | "env";
    patterns: string[];
};

/** `true` / `false`, or a per-feature map like `{ capabilities: true }`. */
type ExperimentalValue = boolean | Record<string, boolean>;

interface ExperimentalSpecOptions {
    /** The `enable_experimental` value. Omit to leave it unset (the default). */
    enableExperimental?: ExperimentalValue;
    /** Generator-level capability policy. */
    capabilities?: Capability[];
    /** The generator script body. */
    script: string;
}

// Writes are allowed only within `@project` (the `-o out` dir). Writing
// `../escaped.txt` lands in the workspace root, outside the allow-list: denied
// when capability enforcement is on, committed when the feature is off (the
// script then runs unconfined and the policy is not applied).
const FS_SCOPED: Capability[] = [
    { access: "allow", domain: "fs.read", patterns: ["@project/**"] },
    { access: "allow", domain: "fs.write", patterns: ["@project/**"] },
];

const WRITE_OUTSIDE_SCRIPT = `export default async function (ctx) {
    await ctx.sys.fs.writeStringToFile("../escaped.txt", "leak");
}`;

function experimentalSpec(opts: ExperimentalSpecOptions): WorkspaceSpec {
    const generator: Record<string, unknown> = {
        name: "capgen",
        description: "experimental gating generator",
        actions: [{ type: "run-javascript", script: "gen.mjs", data: {} }],
    };
    if (opts.capabilities) {
        generator.capabilities = { rules: opts.capabilities };
    }

    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
            ...(opts.enableExperimental !== undefined
                ? { enable_experimental: opts.enableExperimental }
                : {}),
        },
        projects: {
            "generators/capgen/generator.omni.yaml": generator,
        },
        files: {
            ".omni/sources/generator/.keep": "",
            "generators/capgen/gen.mjs": opts.script,
        },
    };
}

function runCapgen(ws: ReturnType<typeof makeWorkspace>) {
    return runOmni(
        [
            "generator",
            "run",
            "-n",
            "capgen",
            "-o",
            "out",
            "--use-defaults",
            "--save-session=false",
        ],
        { cwd: ws.cwd },
    );
}

describe("+generator @e2e (experimental: capabilities gate)", {
    tags: ["generator"],
    timeout: 60_000,
}, () => {
    it("off by default: a write outside the declared allow-list is committed (unconfined)", async () => {
        const ws = makeWorkspace(
            experimentalSpec({
                capabilities: FS_SCOPED,
                script: WRITE_OUTSIDE_SCRIPT,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveSucceeded();
        expect(ws.read("escaped.txt")).toBe("leak");
        expect(result).toOutputContaining(
            "capabilities feature is experimental and disabled",
        );
    });

    it("enforces capabilities when enabled via the boolean form", async () => {
        const ws = makeWorkspace(
            experimentalSpec({
                enableExperimental: true,
                capabilities: FS_SCOPED,
                script: WRITE_OUTSIDE_SCRIPT,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveFailed();
        expect(ws.exists("escaped.txt")).toBe(false);
    });

    it("enforces capabilities when toggled on via the per-feature map", async () => {
        const ws = makeWorkspace(
            experimentalSpec({
                enableExperimental: { capabilities: true },
                capabilities: FS_SCOPED,
                script: WRITE_OUTSIDE_SCRIPT,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveFailed();
        expect(ws.exists("escaped.txt")).toBe(false);
    });

    it("does not enforce when the capabilities feature is explicitly off", async () => {
        const ws = makeWorkspace(
            experimentalSpec({
                enableExperimental: { capabilities: false },
                capabilities: FS_SCOPED,
                script: WRITE_OUTSIDE_SCRIPT,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveSucceeded();
        expect(ws.read("escaped.txt")).toBe("leak");
        expect(result).toOutputContaining(
            "capabilities feature is experimental and disabled",
        );
    });

    it("an unrelated experimental toggle does not enable capabilities", async () => {
        const ws = makeWorkspace(
            experimentalSpec({
                enableExperimental: { "some-other-feature": true },
                capabilities: FS_SCOPED,
                script: WRITE_OUTSIDE_SCRIPT,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveSucceeded();
        expect(ws.read("escaped.txt")).toBe("leak");
    });
});
