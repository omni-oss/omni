/**
 * Capability enforcement for `run-javascript` generator actions.
 *
 * A generator script runs under a cascading, fail-closed capability policy
 * (workspace floor ⧺ generator ⧺ action, deny-dominant). Enforcement has two
 * layers that this suite exercises end to end:
 *
 *   - Filesystem access through `ctx.sys.fs` is brokered in-process by the host
 *     (`PolicyEnforcingSys`) regardless of runtime, so those checks are
 *     deterministic and are asserted under the auto-detected runtime.
 *   - `net` and `process` are confined per runtime: Deno/Node lower (part of)
 *     the policy into launch flags, and a script-level shim narrows whatever the
 *     flags cannot express (all of it, for Bun). The *observable* outcome — a
 *     denied host/program fails, an allowed one proceeds — must hold on every
 *     runtime, so those are parameterized across whichever runtimes are present.
 *
 * Pinned to `crates/omni_generator/src/script_runner.rs`,
 * `crates/omni_capability_enforcement/*`, and the TS shim under
 * `packages/bridge-rpc-services/src/exec-generator-script/enforcement/*`.
 *
 * These tests require a JS runtime (node/bun/deno) on PATH.
 */

import { spawnSync } from "node:child_process";
import { describe, expect, it, type TestContext } from "vitest";
import {
    makeWorkspace,
    runOmni,
    type Workspace,
    type WorkspaceSpec,
} from "@/harness";

type Runtime = "node" | "bun" | "deno";
const RUNTIMES: readonly Runtime[] = ["node", "bun", "deno"];

// Runtimes whose *allow* network path is asserted end to end. All three confine
// `net` and complete an allowed request: Deno/Node lower it into `--allow-net`
// (Node needs v24+ for network permissions; older Node is refused with a clear
// message before launch) and Bun relies on the shim. The positive path is
// asserted **hermetically** against an in-script HTTP server on loopback (no
// external network / DNS), so it is deterministic in CI. The one wrinkle is the
// Windows AppContainer floor, which blocks loopback for node/deno (see
// `loopbackBlockedByFloor`); those cases skip with a named reason rather than
// depending on a real remote host whose reachability the probe cannot predict
// for the confined child.
const NET_ALLOW_RUNTIMES: readonly Runtime[] = ["node", "bun", "deno"];

// Runtimes whose positive `process`-spawn path is asserted end to end. Launching
// a confined child works on all three: the OS sandbox grants the allowed
// program's binary directory (Landlock exec) and the runtime's *real* binary
// directory (resolved via `process.execPath`, so version-manager shims that
// re-exec a cached binary still launch under the sandbox), the shim drops a
// non-existent virtual cwd, and a minimal env allow-list is passed.
const SPAWN_ALLOW_RUNTIMES: readonly Runtime[] = ["node", "bun", "deno"];

// Runtimes where a *direct* `node:child_process` import (bypassing
// `ctx.sys.proc.spawn`) is authorized for the ASYNC family
// (`spawn`/`exec`/`execFile`/`fork`). These all funnel through the shared
// `ChildProcess.prototype.spawn` chokepoint, and patching a shared prototype
// propagates across module graphs on every runtime — including Bun, whose ESM
// named-binding snapshotting defeats the exports patch but not a prototype one
// (the same mechanism as the raw-socket `Socket.prototype.connect` fix). So the
// async family is closed on all three.
const CHILD_PROCESS_ASYNC_PATCH_RUNTIMES: readonly Runtime[] = [
    "node",
    "deno",
    "bun",
];

// Runtimes where a *direct* import of the SYNC family (`spawnSync`/`execSync`/
// `execFileSync`) is authorized. The sync functions create no `ChildProcess`
// and so bypass the prototype chokepoint; they are enforced by the in-process
// exports patch, which reaches all three runtimes — including Bun — because the
// bridge keeps its startup graph free of any eager `import` of
// `node:child_process` (Bun snapshots ESM bindings at first import, so the patch
// must, and does, run before it).
const CHILD_PROCESS_SYNC_PATCH_RUNTIMES: readonly Runtime[] = [
    "node",
    "deno",
    "bun",
];

/** Whether a JS runtime binary is callable on PATH (for gating tests). */
function runtimeAvailable(bin: string): boolean {
    try {
        return spawnSync(bin, ["--version"], { stdio: "ignore" }).status === 0;
    } catch {
        return false;
    }
}

// Whether the resolved `node` actually implements the `--allow-net` permission
// flag. Node did not ship it uniformly across the v24.x line (some builds reject
// it as an unknown option), so feature-detect it directly. When it is absent,
// omni refuses a net-granting generator on Node before launch, so the positive
// `net`-on-Node paths (and the launch-flag net floor) are skipped rather than
// failed — the same treatment an unavailable runtime gets.
let _nodeSupportsNet: boolean | undefined;
function nodeSupportsNet(): boolean {
    if (_nodeSupportsNet !== undefined) {
        return _nodeSupportsNet;
    }

    const MAX_TRIES = 3;

    let exitCode = 0;
    let tries = 0;

    while (exitCode !== 0 && tries <= MAX_TRIES) {
        try {
            const r = spawnSync("node", ["--help"], { encoding: "utf8" });
            const supportsNet =
                r.status === 0 && /--allow-net\b/.test(r.stdout ?? "");
            exitCode = r.status ?? 1;

            _nodeSupportsNet = supportsNet;
        } catch {
            _nodeSupportsNet = false;
        }

        ++tries;
    }
    _nodeSupportsNet ??= false;

    return _nodeSupportsNet;
}

/** Whether the host running the tests is Windows. */
const IS_WINDOWS = process.platform === "win32";

/**
 * Whether loopback (127.0.0.1 / localhost) is unreachable for `rt` because of
 * its OS-sandbox floor. On Windows, `node`/`deno` run inside an AppContainer
 * whose default-deny network is re-opened only for *internet* client
 * capabilities (`internetClient`), never loopback — reaching the local machine
 * needs a separate admin-only exemption the sandbox deliberately does not grant
 * (see `crates/omni_capability_enforcement/src/appcontainer_sandbox.rs`). So a
 * *positive* loopback assertion cannot pass there; it is skipped, not failed
 * (the honest treatment of a floor that blocks the very thing the case needs).
 * `bun` has no AppContainer on Windows, so its loopback still works.
 */
function loopbackBlockedByFloor(rt: Runtime): boolean {
    return IS_WINDOWS && (rt === "node" || rt === "deno");
}

/**
 * The reason `rt` cannot run a case with the given needs, or `null` to run it.
 * Consolidates the three recurring per-runtime gates so a skip is decided in one
 * place instead of being re-implemented as a silent early return per test.
 */
function perRuntimeSkipReason(
    rt: Runtime,
    opts: { needsNodeNet?: boolean; needsLoopback?: boolean },
): string | null {
    if (!runtimeAvailable(rt)) {
        return `${rt} not on PATH`;
    }
    if (opts.needsNodeNet && rt === "node" && !nodeSupportsNet()) {
        return "node lacks the --allow-net permission flag";
    }
    if (opts.needsLoopback && loopbackBlockedByFloor(rt)) {
        return "AppContainer blocks loopback on Windows for node/deno";
    }
    return null;
}

/**
 * Declare a per-runtime `it` whose skip conditions are **named and visible** in
 * the report: a silent `ctx.skip()` early return hides *why* a case did not run,
 * so a runtime/flag/floor gap looks the same as a pass. This bakes the reason
 * into the (skipped) test title instead. `body` only runs when the case is
 * eligible on `rt`.
 */
function itPerRuntime(
    runtimes: readonly Runtime[],
    opts: {
        title: (rt: Runtime) => string;
        needsNodeNet?: boolean;
        needsLoopback?: boolean;
    },
    body: (rt: Runtime, ctx: TestContext) => Promise<void>,
): void {
    for (const rt of runtimes) {
        const reason = perRuntimeSkipReason(rt, opts);
        const title = reason
            ? `${opts.title(rt)} (skipped: ${reason})`
            : opts.title(rt);
        it.skipIf(reason !== null)(title, (ctx) => body(rt, ctx));
    }
}

type Capability = {
    access: "allow" | "deny";
    domain: "fs.read" | "fs.write" | "net" | "process" | "env";
    patterns: string[];
};

interface CapSpecOptions {
    /** The generator script body (an `export default async function (ctx)`). */
    script: string;
    /** Generator-level capability policy. Omit for the built-in confined floor. */
    capabilities?: Capability[];
    /** Data handed to the script as `ctx.data`. */
    data?: Record<string, unknown>;
    /** Pin a specific runtime; otherwise auto-detected. */
    runtime?: Runtime;
    /**
     * Generator-level floor-gap stance. `require-floor` turns a governed domain
     * with no un-bypassable floor from a warning into a hard refusal.
     */
    strictness?: "warn" | "require-floor";
    /** Extra workspace files (keyed by workspace-relative path). */
    extraFiles?: Record<string, string>;
    /**
     * Workspace-level capability ceiling (the outermost policy level). Under the
     * shrink-only model a generator can only *narrow* this, never widen it.
     */
    workspaceCapabilities?: Capability[];
}

/**
 * A workspace with a single `capgen` generator that runs {@link
 * CapSpecOptions.script} through one `run-javascript` action under the given
 * capability policy.
 */
function capGeneratorSpec(opts: CapSpecOptions): WorkspaceSpec {
    const action: Record<string, unknown> = {
        type: "run-javascript",
        script: "gen.mjs",
        data: opts.data ?? {},
    };
    if (opts.runtime) {
        action.runtime = opts.runtime;
    }

    const generator: Record<string, unknown> = {
        name: "capgen",
        description: "capability enforcement generator",
        actions: [action],
    };
    // Capabilities are always an object of `{ rules, strictness }`.
    if (opts.capabilities || opts.strictness) {
        generator.capabilities = {
            ...(opts.capabilities ? { rules: opts.capabilities } : {}),
            ...(opts.strictness ? { strictness: opts.strictness } : {}),
        };
    }

    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
            ...(opts.workspaceCapabilities
                ? { capabilities: { rules: opts.workspaceCapabilities } }
                : {}),
        },
        projects: {
            "generators/capgen/generator.omni.yaml": generator,
        },
        files: {
            ".omni/sources/generator/.keep": "",
            "generators/capgen/gen.mjs": opts.script,
            ...(opts.extraFiles ?? {}),
        },
    };
}

function runCapgen(ws: Workspace) {
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

// The output dir (`-o out`) is `@project`; the workspace root is `@workspace`.
// The script's working directory is the output dir, so relative paths resolve
// there and `../x` escapes into the workspace but outside `@project`.
const FS_SCOPED: Capability[] = [
    { access: "allow", domain: "fs.read", patterns: ["@project/**"] },
    { access: "allow", domain: "fs.write", patterns: ["@project/**"] },
];

describe("+generator @e2e (capabilities: filesystem)", {
    tags: ["generator"],
    timeout: 60_000,
}, () => {
    it("permits writing within the declared allow-list", async () => {
        const ws = makeWorkspace(
            capGeneratorSpec({
                capabilities: FS_SCOPED,
                script: `export default async function (ctx) {
                    await ctx.sys.fs.writeStringToFile("allowed.txt", "ok");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveSucceeded();
        expect(ws.read("out/allowed.txt")).toBe("ok");
    });

    it("denies writing outside the declared allow-list", async () => {
        const ws = makeWorkspace(
            capGeneratorSpec({
                capabilities: FS_SCOPED,
                // `../escaped.txt` lands in the workspace root, outside `@project`.
                script: `export default async function (ctx) {
                    await ctx.sys.fs.writeStringToFile("../escaped.txt", "leak");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveFailed();
        expect(ws.exists("escaped.txt")).toBe(false);
    });

    it("denies reading a file outside the declared allow-list", async () => {
        const ws = makeWorkspace(
            capGeneratorSpec({
                capabilities: FS_SCOPED,
                extraFiles: { "secret.txt": "topsecret" },
                // `../secret.txt` is in the workspace but outside `@project`.
                script: `export default async function (ctx) {
                    await ctx.sys.fs.readFileAsString("../secret.txt");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveFailed();
    });

    it("lets a deny rule dominate an overlapping allow (deny-dominant)", async () => {
        const ws = makeWorkspace(
            capGeneratorSpec({
                capabilities: [
                    {
                        access: "allow",
                        domain: "fs.write",
                        patterns: ["@project/**"],
                    },
                    {
                        access: "deny",
                        domain: "fs.write",
                        patterns: ["@project/secret/**"],
                    },
                ],
                script: `export default async function (ctx) {
                    await ctx.sys.fs.writeStringToFile("secret/blocked.txt", "leak");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveFailed();
        expect(ws.exists("out/secret/blocked.txt")).toBe(false);
    });

    it("still applies the allow outside the denied subpath", async () => {
        const ws = makeWorkspace(
            capGeneratorSpec({
                capabilities: [
                    {
                        access: "allow",
                        domain: "fs.write",
                        patterns: ["@project/**"],
                    },
                    {
                        access: "deny",
                        domain: "fs.write",
                        patterns: ["@project/secret/**"],
                    },
                ],
                script: `export default async function (ctx) {
                    await ctx.sys.fs.writeStringToFile("public.txt", "fine");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveSucceeded();
        expect(ws.read("out/public.txt")).toBe("fine");
    });

    it("commits a capability-free script's in-workspace writes (built-in floor)", async () => {
        // No declared policy → the confined default floor: read/write anywhere
        // in the workspace, but no net/process and nothing outside it.
        const ws = makeWorkspace(
            capGeneratorSpec({
                script: `export default async function (ctx) {
                    await ctx.sys.fs.writeStringToFile("floor.txt", "floored");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveSucceeded();
        expect(ws.read("out/floor.txt")).toBe("floored");
    });

    it("makes a declared policy exhaustive: no implicit filesystem floor", async () => {
        // Declaring *any* capability opts out of the default floor entirely, so a
        // net-only policy grants no filesystem access — writing must be denied.
        const ws = makeWorkspace(
            capGeneratorSpec({
                capabilities: [
                    {
                        access: "allow",
                        domain: "net",
                        patterns: ["github.com:443"],
                    },
                ],
                script: `export default async function (ctx) {
                    await ctx.sys.fs.writeStringToFile("should-fail.txt", "nope");
                }`,
            }),
        );

        const result = await runCapgen(ws);

        expect(result).toHaveFailed();
        expect(ws.exists("out/should-fail.txt")).toBe(false);
    });
});

describe("+generator @e2e (capabilities: env)", {
    tags: ["generator"],
    timeout: 90_000,
}, () => {
    // `env` is enforced on two fronts that must agree: the host broker filters
    // the RPC environment snapshot by variable *name* (default
    // `EnvAccess::Filter`), and — because `env` is now a shim domain — the same
    // layered rules are handed to the JS side, which wraps `ctx.sys.proc.env()`
    // in a capability-filtered view. Deno can lower a *literal* allow into
    // `--allow-env`; a glob (`PUBLIC_*`) it cannot express, so that case is
    // enforced by the shim on every runtime. Node/Bun have no env launch flag,
    // so the shim always owns `env` there. `-i` surfaces the parent process env
    // so the injected vars reach the resolved env the generator reads.
    const envAllowingCaps = (rules: Capability[]): Capability[] => [
        { access: "allow", domain: "fs.read", patterns: ["@project/**"] },
        { access: "allow", domain: "fs.write", patterns: ["@project/**"] },
        ...rules,
    ];

    const READ_ENV_SCRIPT = `export default async function (ctx) {
        const env = ctx.sys.proc.env().toObject();
        await ctx.sys.fs.writeStringToFile("env.json", JSON.stringify(env));
    }`;

    function runCapgenWithEnv(ws: Workspace, env: Record<string, string>) {
        return runOmni(
            [
                "-i",
                "generator",
                "run",
                "-n",
                "capgen",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd, env },
        );
    }

    function readEnvJson(ws: Workspace): Record<string, string> {
        return JSON.parse(ws.read("out/env.json")) as Record<string, string>;
    }

    for (const rt of RUNTIMES) {
        it(`${rt}: filters the snapshot to the policy-allowed names`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: envAllowingCaps([
                        {
                            access: "allow",
                            domain: "env",
                            patterns: ["ALLOWED_VAR"],
                        },
                        {
                            access: "allow",
                            domain: "env",
                            patterns: ["PUBLIC_*"],
                        },
                    ]),
                    script: READ_ENV_SCRIPT,
                }),
            );

            const result = await runCapgenWithEnv(ws, {
                ALLOWED_VAR: "yes",
                PUBLIC_TOKEN: "pub",
                SECRET_KEY: "nope",
            });

            expect(result).toHaveSucceeded();
            const seen = readEnvJson(ws);
            expect(seen.ALLOWED_VAR).toBe("yes");
            expect(seen.PUBLIC_TOKEN).toBe("pub");
            expect(seen.SECRET_KEY).toBeUndefined();
        });

        it(`${rt}: lets a deny rule dominate an overlapping env allow (deny-dominant)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: envAllowingCaps([
                        {
                            access: "allow",
                            domain: "env",
                            patterns: ["PUBLIC_*"],
                        },
                        {
                            access: "deny",
                            domain: "env",
                            patterns: ["PUBLIC_SECRET"],
                        },
                    ]),
                    script: READ_ENV_SCRIPT,
                }),
            );

            const result = await runCapgenWithEnv(ws, {
                PUBLIC_OK: "fine",
                PUBLIC_SECRET: "leak",
            });

            expect(result).toHaveSucceeded();
            const seen = readEnvJson(ws);
            expect(seen.PUBLIC_OK).toBe("fine");
            expect(seen.PUBLIC_SECRET).toBeUndefined();
        });

        it(`${rt}: an env-free declared policy hides all vars (exhaustive)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            // Declaring only fs capabilities opts out of any implicit env floor,
            // so no variable name is granted and the snapshot the script sees is
            // empty of the injected vars.
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "fs.read",
                            patterns: ["@project/**"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: READ_ENV_SCRIPT,
                }),
            );

            const result = await runCapgenWithEnv(ws, {
                ALLOWED_VAR: "yes",
                SECRET_KEY: "nope",
            });

            expect(result).toHaveSucceeded();
            const seen = readEnvJson(ws);
            expect(seen.ALLOWED_VAR).toBeUndefined();
            expect(seen.SECRET_KEY).toBeUndefined();
        });
    }
});

// ─────────────────────────────────────────────────────────────────────────────
// Data-driven truth table for RAW environment reads (`Deno.env` / `process.env`)
// — the un-mediated floor a script hits when it bypasses `ctx.sys.proc.env()`.
// (The RPC-filtered view above is uniformly fail-closed on every runtime; this
// block covers what the *runtime itself* enforces.)
//
// Model under test:
//   * A fixed, non-sensitive BOOTSTRAP set (PATH/HOME/locale/temp/…, incl. `TZ`)
//     is kept in the child's environment by the spawn-time scrub so ordinary
//     tools keep working. On Deno it is granted via `--allow-env` ONLY when
//     `process` is allowed (a child needs it) and MINUS any key the `env` policy
//     explicitly denies ("deny-subtract", including globbed denies Deno cannot
//     express as `--deny-env`). Every other variable is fail-closed (needs an
//     explicit `allow`).
//   * Deno has a real per-variable raw-read gate (`--allow-env`), so the model is
//     enforced there. Node/Bun have NO raw-read gate, so the only floor is the
//     scrub: bootstrap keys stay readable (even when denied — the scrub re-adds
//     them unconditionally), and unmentioned/denied non-bootstrap vars are simply
//     absent.
//
// Raw-read outcome vocabulary (per probed variable):
//   * "value:<v>" — read succeeded, returned <v>
//   * "unset"     — read succeeded, variable absent (undefined)
//   * "denied"    — the read threw (Deno's permission gate)
// ─────────────────────────────────────────────────────────────────────────────
describe("+generator @e2e (capabilities: env raw-read floor)", {
    tags: ["generator"],
    timeout: 90_000,
}, () => {
    // Raw-reads one variable (`ctx.data.probe`) via the un-mediated runtime API,
    // recording the three-way outcome so a Deno permission throw is observable
    // rather than crashing the run.
    const RAW_PROBE_SCRIPT = `export default async function (ctx) {
            const name = ctx.data.probe;
            let outcome;
            try {
                const denoEnv = globalThis.Deno?.env;
                const value = denoEnv
                    ? denoEnv.get(name)
                    : globalThis.process?.env?.[name];
                outcome = value === undefined ? "unset" : "value:" + value;
            } catch {
                outcome = "denied";
            }
            await ctx.sys.fs.writeStringToFile("env-probe.json", JSON.stringify({ outcome }));
        }`;

    type Outcome = string; // "value:<v>" | "unset" | "denied"

    interface RawEnvCase {
        /** Test title. */
        readonly name: string;
        /** Generator capability policy (already includes fs scope). */
        readonly capabilities: Capability[];
        /** Variables injected into the run's ambient environment. */
        readonly inject: Record<string, string>;
        /** The variable the script raw-reads. */
        readonly probe: string;
        /** Expected raw-read outcome per runtime. */
        readonly expected: Record<Runtime, Outcome>;
    }

    const allowProcess: Capability = {
        access: "allow",
        domain: "process",
        patterns: ["node"],
    };

    // `TZ` is in BOTH the scrub bootstrap set and Deno's `--allow-env` bootstrap
    // grant, so it is the canonical "bootstrap key" probe. `PROBE_SECRET` is a
    // non-bootstrap name (scrubbed out unless explicitly allowed).
    const CASES: readonly RawEnvCase[] = [
        {
            name: "an explicitly-allowed var is readable on every runtime",
            capabilities: [
                ...FS_SCOPED,
                { access: "allow", domain: "env", patterns: ["PROBE_ALLOWED"] },
            ],
            inject: { PROBE_ALLOWED: "allowed-val" },
            probe: "PROBE_ALLOWED",
            expected: {
                node: "value:allowed-val",
                bun: "value:allowed-val",
                deno: "value:allowed-val",
            },
        },
        {
            name: "a bootstrap key is readable when `process` is allowed (deny-subtract grant)",
            capabilities: [...FS_SCOPED, allowProcess],
            inject: { TZ: "boot-val" },
            probe: "TZ",
            expected: {
                node: "value:boot-val",
                bun: "value:boot-val",
                deno: "value:boot-val",
            },
        },
        {
            name: "a bootstrap key WITHOUT `process`: Deno fail-closes, Node/Bun stay ungated",
            capabilities: [...FS_SCOPED],
            inject: { TZ: "boot-val" },
            probe: "TZ",
            expected: {
                node: "value:boot-val",
                bun: "value:boot-val",
                deno: "denied",
            },
        },
        {
            name: "a literal deny of a bootstrap key is honored only on Deno",
            capabilities: [
                ...FS_SCOPED,
                allowProcess,
                { access: "deny", domain: "env", patterns: ["TZ"] },
            ],
            inject: { TZ: "boot-val" },
            probe: "TZ",
            expected: {
                node: "value:boot-val",
                bun: "value:boot-val",
                deno: "denied",
            },
        },
        {
            name: "a globbed deny of a bootstrap key is deny-subtracted on Deno",
            capabilities: [
                ...FS_SCOPED,
                allowProcess,
                { access: "deny", domain: "env", patterns: ["T*"] },
            ],
            inject: { TZ: "boot-val" },
            probe: "TZ",
            expected: {
                node: "value:boot-val",
                bun: "value:boot-val",
                deno: "denied",
            },
        },
        {
            name: "an unmentioned non-bootstrap var is removed by the scrub (no value anywhere)",
            capabilities: [...FS_SCOPED],
            inject: { PROBE_SECRET: "secret-val" },
            probe: "PROBE_SECRET",
            expected: { node: "unset", bun: "unset", deno: "denied" },
        },
        {
            name: "an explicitly-denied non-bootstrap var is unreadable everywhere",
            capabilities: [
                ...FS_SCOPED,
                allowProcess,
                { access: "deny", domain: "env", patterns: ["PROBE_SECRET"] },
            ],
            inject: { PROBE_SECRET: "secret-val" },
            probe: "PROBE_SECRET",
            expected: { node: "unset", bun: "unset", deno: "denied" },
        },
    ];

    for (const rt of RUNTIMES) {
        for (const c of CASES) {
            it(`${rt}: ${c.name}`, async (ctx) => {
                if (!runtimeAvailable(rt)) {
                    ctx.skip();
                    return;
                }
                const ws = makeWorkspace(
                    capGeneratorSpec({
                        runtime: rt,
                        capabilities: c.capabilities,
                        data: { probe: c.probe },
                        script: RAW_PROBE_SCRIPT,
                    }),
                );

                const result = await runOmni(
                    [
                        "-i",
                        "generator",
                        "run",
                        "-n",
                        "capgen",
                        "-o",
                        "out",
                        "--use-defaults",
                        "--save-session=false",
                    ],
                    { cwd: ws.cwd, env: c.inject },
                );

                // The script never throws (it records the outcome), so the run
                // always succeeds; the enforcement decision is in the payload.
                expect(result).toHaveSucceeded();
                const { outcome } = JSON.parse(
                    ws.read("out/env-probe.json"),
                ) as { outcome: Outcome };
                expect(outcome).toBe(c.expected[rt]);
            });
        }
    }
});

describe("+generator @e2e (capabilities: network)", {
    tags: ["generator"],
    timeout: 90_000,
}, () => {
    for (const rt of RUNTIMES) {
        it(`${rt}: denies network under the confined floor (ctx.sys fetch)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    script: `export default async function (ctx) {
                        await ctx.sys.net.http.fetch("http://blocked.invalid/");
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });

        it(`${rt}: denies network via the ambient global fetch too`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    script: `export default async function () {
                        await fetch("http://blocked.invalid/");
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });

        it(`${rt}: denies a host outside the net allow-list`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "net",
                            patterns: ["github.com:443"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `export default async function (ctx) {
                        await ctx.sys.net.http.fetch("https://denied.invalid/");
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });

        it(`${rt}: denies a raw socket that bypasses fetch (direct node:net)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            // A script can skip `ctx.sys`/`fetch` and open a raw TCP socket
            // directly. The in-process builtin patch must still authorize it:
            // here the policy allows only `example.com:443`, so a raw connect to
            // a *live* loopback server (which the coarse Node `--allow-net` /
            // Bun's absent model would happily permit) must be refused. If the
            // patch failed, the connect would succeed and the run would pass —
            // so a green run here is a real regression signal.
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "net",
                            patterns: ["example.com:443"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `import net from "node:net";
                    import http from "node:http";
                    export default async function () {
                        const server = http.createServer((_q, r) => { r.writeHead(200); r.end("hi"); });
                        await new Promise((res) => server.listen(0, "127.0.0.1", res));
                        const { port } = server.address();
                        try {
                            await new Promise((resolve, reject) => {
                                const s = net.connect(port, "127.0.0.1");
                                s.on("connect", () => { s.end(); resolve(); });
                                s.on("error", reject);
                            });
                        } finally {
                            server.close();
                        }
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });

        it(`${rt}: a generator cannot widen net past the workspace ceiling`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            // Shrink-only (attenuation): the workspace ceiling allows only
            // example.com; the generator *tries* to widen it by also allowing
            // loopback. A child may only narrow the ceiling it inherits, so the
            // loopback grant is capped away even though the generator lists it.
            //
            // The script fetches a *live* in-script loopback server, so the run
            // can only fail because enforcement refused the connection — not
            // because the host was unreachable. Two levels now constrain `net`,
            // so the shim folds them per level on every runtime (on Deno the
            // coarse --allow-net would otherwise permit the union, so a green
            // run here is a real escalation-regression signal).
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    workspaceCapabilities: [
                        {
                            access: "allow",
                            domain: "net",
                            patterns: ["example.com:443"],
                        },
                    ],
                    capabilities: [
                        {
                            access: "allow",
                            domain: "net",
                            patterns: ["example.com:443", "127.0.0.1:*"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `import http from "node:http";
                    export default async function (ctx) {
                        const server = http.createServer((_q, r) => {
                            r.writeHead(200);
                            r.end("hello");
                        });
                        await new Promise((res) => server.listen(0, "127.0.0.1", res));
                        const { port } = server.address();
                        try {
                            await ctx.sys.net.http.fetch(\`http://127.0.0.1:\${port}/\`);
                        } finally {
                            server.close();
                        }
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });

        it(`${rt}: a generator keeps the net access the ceiling still grants`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            if (rt === "node" && !nodeSupportsNet()) {
                ctx.skip();
                return;
            }
            if (loopbackBlockedByFloor(rt)) {
                // AppContainer blocks loopback on Windows for node/deno, so the
                // allowed loopback call cannot complete under the floor.
                ctx.skip();
                return;
            }
            // The intersection still permits an allowed call: loopback is in
            // both the workspace ceiling and the generator's (narrowed) policy,
            // so an allowed request completes end to end even with the layered
            // fold active.
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    workspaceCapabilities: [
                        {
                            access: "allow",
                            domain: "net",
                            patterns: ["127.0.0.1:*"],
                        },
                    ],
                    capabilities: [
                        {
                            access: "allow",
                            domain: "net",
                            patterns: ["127.0.0.1:*"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `import http from "node:http";
                    export default async function (ctx) {
                        const server = http.createServer((_q, r) => {
                            r.writeHead(200);
                            r.end("hello");
                        });
                        await new Promise((res) => server.listen(0, "127.0.0.1", res));
                        const { port } = server.address();
                        try {
                            const res = await ctx.sys.net.http.fetch(\`http://127.0.0.1:\${port}/\`);
                            await ctx.sys.fs.writeStringToFile("ceil.txt", String(res.status));
                        } finally {
                            server.close();
                        }
                    }`,
                }),
            );

            const result = await runCapgen(ws);
            expect(result).toHaveSucceeded();
            expect(ws.read("out/ceil.txt")).toBe("200");
        });
    }

    for (const rt of NET_ALLOW_RUNTIMES) {
        itPerRuntime(
            [rt],
            {
                title: (r) =>
                    `${r}: permits an allowed host over loopback (no external network)`,
                needsNodeNet: true,
                needsLoopback: true,
            },
            async (_rt, ctx) => {
                void ctx;
                // Deterministic: the script starts its own HTTP server on
                // 127.0.0.1 and fetches it through the enforced
                // `ctx.sys.net.http.fetch`. The `net` policy allows
                // `127.0.0.1:*`, so an allowed request is exercised end to end
                // without depending on external reachability.
                const ws = makeWorkspace(
                    capGeneratorSpec({
                        runtime: rt,
                        capabilities: [
                            {
                                access: "allow",
                                domain: "net",
                                patterns: ["127.0.0.1:*"],
                            },
                            {
                                access: "allow",
                                domain: "fs.write",
                                patterns: ["@project/**"],
                            },
                        ],
                        script: `import http from "node:http";
                    export default async function (ctx) {
                        const server = http.createServer((_q, r) => {
                            r.writeHead(200, { "content-type": "text/plain" });
                            r.end("hello");
                        });
                        await new Promise((res) => server.listen(0, "127.0.0.1", res));
                        const { port } = server.address();
                        try {
                            const res = await ctx.sys.net.http.fetch(\`http://127.0.0.1:\${port}/\`);
                            const body = await res.text();
                            await ctx.sys.fs.writeStringToFile("loopback.txt", \`\${res.status} \${body}\`);
                        } finally {
                            server.close();
                        }
                    }`,
                    }),
                );

                const result = await runCapgen(ws);

                expect(result).toHaveSucceeded();
                expect(ws.read("out/loopback.txt")).toBe("200 hello");
            },
        );

        itPerRuntime(
            [rt],
            {
                title: (r) => `${r}: permits a host in the net allow-list`,
                needsNodeNet: true,
                needsLoopback: true,
            },
            async (_rt, ctx) => {
                void ctx;
                // The named-host analog of the IP-literal loopback test above:
                // it allows `localhost:*` (a *name*, resolved locally with no
                // external DNS) and fetches an in-script server, so the
                // allow-list + name-resolution path is exercised hermetically
                // — no dependency on a real remote host whose reachability the
                // unconfined probe cannot predict for the sandboxed child.
                const ws = makeWorkspace(
                    capGeneratorSpec({
                        runtime: rt,
                        capabilities: [
                            {
                                access: "allow",
                                domain: "net",
                                patterns: ["localhost:*", "127.0.0.1:*"],
                            },
                            {
                                access: "allow",
                                domain: "fs.write",
                                patterns: ["@project/**"],
                            },
                        ],
                        script: `import http from "node:http";
                    export default async function (ctx) {
                        const server = http.createServer((_q, r) => {
                            r.writeHead(204);
                            r.end();
                        });
                        await new Promise((res) => server.listen(0, "127.0.0.1", res));
                        const { port } = server.address();
                        try {
                            const res = await ctx.sys.net.http.fetch(\`http://localhost:\${port}/\`);
                            await ctx.sys.fs.writeStringToFile("net-status.txt", String(res.status));
                        } finally {
                            server.close();
                        }
                    }`,
                    }),
                );

                const result = await runCapgen(ws);

                expect(result).toHaveSucceeded();
                expect(ws.read("out/net-status.txt")).toMatch(/^\d{3}$/);
            },
        );
    }
});

describe("+generator @e2e (capabilities: process)", {
    tags: ["generator"],
    timeout: 90_000,
}, () => {
    for (const rt of RUNTIMES) {
        it(`${rt}: denies spawning under the confined floor`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    script: `export default async function (ctx) {
                        await ctx.sys.proc.spawn("node", { args: ["-e", "0"] });
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });

        it(`${rt}: denies a program outside the process allow-list`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "process",
                            patterns: ["node"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `export default async function (ctx) {
                        await ctx.sys.proc.spawn("git", { args: ["--version"] });
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });
    }

    // A *direct* `node:child_process` import (bypassing `ctx.sys.proc.spawn`)
    // must still be authorized. The ASYNC family funnels through the shared
    // `ChildProcess.prototype.spawn`, which the in-process patch intercepts on
    // every runtime — including Bun, where the exports patch alone would miss a
    // direct `import { spawn }` (Bun snapshots ESM named bindings, but a shared
    // prototype patch still propagates). Raw sockets are closed the same way
    // (see the network suite).
    for (const rt of CHILD_PROCESS_ASYNC_PATCH_RUNTIMES) {
        it(`${rt}: denies a directly-imported async child_process spawn outside the allow-list`, async (ctx) => {
            if (!runtimeAvailable(rt) || !runtimeAvailable("git")) {
                ctx.skip();
                return;
            }
            // Bypass `ctx.sys.proc.spawn` and reach `node:child_process`
            // directly via the async `spawn`. The policy allows only `node`, so
            // spawning the (present) `git` binary must be denied before launch
            // — the patched prototype throws synchronously.
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "process",
                            patterns: ["node"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `import { spawn } from "node:child_process";
                    export default async function () {
                        spawn("git", ["--version"]);
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });
    }

    // The SYNC family (`spawnSync`/…) bypasses the prototype chokepoint, so it
    // relies on the in-process exports patch. That reaches all three runtimes,
    // including Bun: the bridge has no eager `import` of `node:child_process`,
    // so the patch runs before the generator script's first ESM import of it
    // (which is when Bun snapshots the binding).
    for (const rt of CHILD_PROCESS_SYNC_PATCH_RUNTIMES) {
        it(`${rt}: denies a directly-imported child_process spawnSync outside the allow-list`, async (ctx) => {
            if (!runtimeAvailable(rt) || !runtimeAvailable("git")) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "process",
                            patterns: ["node"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `import { spawnSync } from "node:child_process";
                    export default async function () {
                        const r = spawnSync("git", ["--version"]);
                        if (r.error) throw r.error;
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });
    }

    // Shell-injection regression (audit #4): a shell invocation runs an
    // *arbitrary* command line, so the authorized program is the SHELL BINARY
    // itself, never the command line's first token. Granting only the inner
    // program (`git`) must therefore NOT authorize `exec`/`execSync`, closing
    // the classic `exec("git; curl evil")` laundering where the benign first
    // token passed the check while the shell went on to run the rest.
    // (Pre-fix this SUCCEEDED because only `git` was extracted and checked.)
    //
    // `exec` is async and funnels through the shared `ChildProcess.prototype.spawn`
    // chokepoint (which reads the runtime-resolved `file` == the shell), so it is
    // closed on every runtime.
    for (const rt of CHILD_PROCESS_ASYNC_PATCH_RUNTIMES) {
        it(`${rt}: denies exec() shell injection when only the inner program is allowed`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "process",
                            patterns: ["git"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    // `exec` always runs `/bin/sh -c <cmd>`; the shell is not in
                    // the allow-list, so this is refused before launch even
                    // though the command line begins with the allowed `git`.
                    script: `import { exec } from "node:child_process";
                    export default async function () {
                        exec("git --version; echo INJECTED");
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });
    }

    // The SYNC shell form (`execSync`) bypasses the prototype chokepoint and is
    // gated by the in-process exports patch, which authorizes the hardcoded
    // `/bin/sh` (see `defaultShellProgram`). Same deny outcome on all three.
    for (const rt of CHILD_PROCESS_SYNC_PATCH_RUNTIMES) {
        it(`${rt}: denies execSync() shell injection when only the inner program is allowed`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "process",
                            patterns: ["git"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `import { execSync } from "node:child_process";
                    export default async function () {
                        execSync("git --version; echo INJECTED");
                    }`,
                }),
            );

            expect(await runCapgen(ws)).toHaveFailed();
        });
    }

    // Positive shell opt-in (audit #4, the other half): once the SHELL is
    // granted, a shell invocation is permitted and actually runs — proving the
    // shell-as-program model is an opt-in *narrowing*, not a blanket ban. `echo`
    // is a POSIX shell builtin, so `/bin/sh -c "echo …"` spawns no further child.
    //
    // A tight `process: ["/bin/sh"]` grant now works on all three runtimes. Deno
    // additionally needs `env: ["*"]` because its node-compat `execSync`
    // enumerates the environment (`Deno.env.toObject()`, plus vars like
    // `NODE_V8_COVERAGE`) to normalize spawn args, which requires a blanket
    // `--allow-env` (a whole-domain `*` lowers to Deno's value-less
    // `--allow-env`, see deno.rs). Node/Bun don't gate raw env reads.
    //
    // Note the interplay with the env scrub: Deno refuses to spawn under a
    // *scoped* `--allow-run` when a linker-hijack var (`LD_LIBRARY_PATH` /
    // `DYLD_*`) is in the child env. `env: ["*"]` would otherwise sweep the
    // ambient `LD_LIBRARY_PATH` into the child, but the runner's
    // `ENV_INJECTION_DENYLIST` drops those vectors even under an allow-all env
    // policy, so the scoped `process: ["/bin/sh"]` grant is honored here.
    //
    // (The deny cases above still cover Deno: the shim throws before any spawn.)
    for (const rt of CHILD_PROCESS_SYNC_PATCH_RUNTIMES) {
        it(`${rt}: runs a shell invocation once the shell is granted`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            // Windows AppContainer limitation (see also `runs an allowed
            // program`): a confined runtime cannot reliably spawn a grandchild
            // shell. Ambient System32 grandchildren work, but `cmd.exe` as a
            // shell is refused by the OS with access-denied even though its ACL
            // grants ALL APPLICATION PACKAGES — and user-installed shells hit the
            // same wall as other user-dir binaries. A shell invocation goes
            // through the shell, so it stays skipped on Windows; the negative
            // process paths above still run everywhere.
            if (process.platform === "win32") {
                ctx.skip();
                return;
            }
            const capabilities: Capability[] = [
                {
                    access: "allow",
                    domain: "process",
                    patterns: ["/bin/sh"],
                },
                {
                    access: "allow",
                    domain: "fs.write",
                    patterns: ["@project/**"],
                },
            ];
            if (rt === "deno") {
                capabilities.push({
                    access: "allow",
                    domain: "env",
                    patterns: ["*"],
                });
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities,
                    script: `import { execSync } from "node:child_process";
                    export default async function (ctx) {
                        const out = execSync("echo shell-ran").toString();
                        await ctx.sys.fs.writeStringToFile("shell.txt", out);
                    }`,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveSucceeded();
            // Exact match (not a substring): the `execSync` failure message also
            // contains "shell-ran", so assert the file holds *only* the echo
            // output to avoid a false pass on an error string.
            expect(ws.read("out/shell.txt").trim()).toBe("shell-ran");
        });
    }

    // Positive `process` path: an *allowed* program actually runs under full
    // confinement, on every runtime. This exercises the OS-sandbox exec grant
    // (the allowed program's binary directory is granted read/execute so
    // Landlock permits the `execve`), the non-existent-cwd fallback, the minimal
    // env allow-list handed to the child, and the shim capturing its stdout.
    for (const rt of SPAWN_ALLOW_RUNTIMES) {
        it(`${rt}: runs an allowed program and captures its output`, async (ctx) => {
            if (!runtimeAvailable(rt) || !runtimeAvailable("git")) {
                ctx.skip();
                return;
            }
            // Windows AppContainer limitation: a confined runtime (node/deno)
            // reliably spawns *ambient* grandchildren (System32) but not
            // arbitrary *user-installed* binaries. Even with the allowed
            // program's real directory granted read/execute (see
            // `appcontainer_sandbox::program_dirs`) and fully-traversable
            // ancestors, the OS refuses the grandchild CreateProcess with
            // access-denied on locked-down hosts — pointing to a host execution
            // policy (WDAC/AppLocker) or a deeper AppContainer restriction
            // outside omni's control. Bun runs unconfined on Windows and
            // exercises this path; skip the confined runtimes until a
            // non-ACL mechanism (e.g. brokering the spawn from the parent) lands.
            if (process.platform === "win32" && rt !== "bun") {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain: "process",
                            patterns: ["git"],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: `export default async function (ctx) {
                        const r = await ctx.sys.proc.spawn("git", { args: ["--version"] });
                        await ctx.sys.fs.writeStringToFile("git-version.txt", r.stdout ?? "");
                    }`,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveSucceeded();
            expect(ws.read("out/git-version.txt")).toMatch(/git version/);
        });
    }
});

// The un-bypassable enforcement *floor* is the runtime's own launch flags plus
// any OS sandbox — mechanisms a confined script cannot lift from inside its
// runtime. The in-process broker/shim are precise but bypassable (raw sockets,
// direct syscalls, FFI). When a governed domain has no floor for the resolved
// runtime, the planner surfaces a non-fatal warning so the operator knows
// enforcement there is defense-in-depth only.
//
// Floor matrix for the shim-narrowable domains (net/process): Deno floors both
// *precisely* (`--allow-net=host` / `--allow-run=prog`); Node floors neither
// precisely — it has NO network flag at all (so `net` rests only on the
// bypassable broker/shim), and its `--allow-child-process` is all-or-nothing, so
// a *specific* program grant is lowered to the coarse whole-gate floor and
// narrowed to the program by the bypassable shim (a superset floor, see
// COARSE_FLOORED); Bun has no permission model, so neither is floored.
// Platform-independent (no OS sandbox covers net/process anywhere). Pinned to
// omni_capability_enforcement `FloorGap` / plan.rs, the Node/Deno backends, and
// `crates/omni_generator/src/script_runner.rs`.
const FLOOR_WARNING = "un-bypassable floor";
type FloorDomain = "net" | "process";
// Precisely floored (the launch flag equals the exact authority): no floor gap.
const FLOORED: ReadonlyArray<readonly [Runtime, FloorDomain]> = [
    ["deno", "net"],
    ["deno", "process"],
];
// No floor at all for the domain — it rests only on the bypassable broker/shim,
// so the diagnostic says the domain "is enforced only" by them.
const UNFLOORED: ReadonlyArray<readonly [Runtime, FloorDomain]> = [
    ["bun", "net"],
    ["bun", "process"],
    ["node", "net"],
];
// Covered by a floor tier, but only *coarsely*: the launch flag is a strict
// superset of the requested pattern (Node's all-or-nothing
// `--allow-child-process` for a specific program), narrowed to the pattern by
// the bypassable shim. This is a floor gap too — the kernel floor permits more
// than the policy — but its diagnostic explains the *superset* rather than a
// total absence of a floor.
const COARSE_FLOORED: ReadonlyArray<readonly [Runtime, FloorDomain]> = [
    ["node", "process"],
];
const floorPattern = (d: FloorDomain): string =>
    d === "net" ? "127.0.0.1:*" : "git";

describe("+generator @e2e (capabilities: enforcement floor)", {
    tags: ["generator"],
    timeout: 60_000,
}, () => {
    // A script that only writes a file; the floor warning is emitted at plan
    // time, independent of whether the governed domain is actually exercised.
    const writeScript = `export default async function (ctx) {
        await ctx.sys.fs.writeStringToFile("floor.txt", "ran");
    }`;

    for (const [rt, domain] of UNFLOORED) {
        it(`${rt}: warns that governed ${domain} has no un-bypassable floor (still runs)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain,
                            patterns: [floorPattern(domain)],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: writeScript,
                }),
            );

            const result = await runCapgen(ws);

            // Non-fatal: the broker/shim still run as defense in depth.
            expect(result).toHaveSucceeded();
            expect(ws.read("out/floor.txt")).toBe("ran");
            expect(result).toOutputContaining(FLOOR_WARNING);
            expect(result).toOutputContaining(`${domain} is enforced only`);
        });
    }

    // Windows-only: `bun` cannot boot inside an AppContainer (it stats every CWD
    // ancestor up to `C:\`, which a Low-integrity container is denied), so unlike
    // node/deno it launches unconfined and its filesystem rests only on the
    // bypassable in-process broker. That weaker guarantee must be surfaced, not
    // silent.
    it("bun: warns that its filesystem has no OS-sandbox floor on Windows", async (ctx) => {
        if (process.platform !== "win32" || !runtimeAvailable("bun")) {
            ctx.skip();
            return;
        }
        const ws = makeWorkspace(
            capGeneratorSpec({
                runtime: "bun",
                capabilities: [
                    {
                        access: "allow",
                        domain: "fs.write",
                        patterns: ["@project/**"],
                    },
                ],
                script: writeScript,
            }),
        );

        const result = await runCapgen(ws);

        // Non-fatal: the broker still mediates every brokered fs route.
        expect(result).toHaveSucceeded();
        expect(ws.read("out/floor.txt")).toBe("ran");
        expect(result).toOutputContaining("cannot run inside an AppContainer");
        expect(result).toOutputContaining(FLOOR_WARNING);
    });

    for (const [rt, domain] of FLOORED) {
        it(`${rt}: governed ${domain} is floored by launch flags (no floor warning)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain,
                            patterns: [floorPattern(domain)],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: writeScript,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveSucceeded();
            expect(ws.read("out/floor.txt")).toBe("ran");
            expect(result.stdout).not.toContain(FLOOR_WARNING);
        });
    }

    // `capabilities: { strictness: require-floor }` promotes a floor gap from a
    // warning to a hard refusal: the run fails before the script executes and
    // nothing is written. Deterministic for every unfloored (runtime, domain).
    for (const [rt, domain] of UNFLOORED) {
        it(`${rt}: require-floor refuses when governed ${domain} has no floor`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    strictness: "require-floor",
                    capabilities: [
                        {
                            access: "allow",
                            domain,
                            patterns: [floorPattern(domain)],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: writeScript,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveFailed();
            expect(ws.exists("out/floor.txt")).toBe(false);
            expect(result).toHaveStderrContaining(
                "un-bypassable enforcement floor",
            );
            expect(result).toHaveStderrContaining(domain);
        });
    }

    // The stronger stance is a no-op when the governed domain is already
    // floored: the launch flags cover it, so require-floor still proceeds and
    // writes.
    for (const [rt, domain] of FLOORED) {
        it(`${rt}: require-floor still runs when ${domain} is floored by launch flags`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    strictness: "require-floor",
                    capabilities: [
                        {
                            access: "allow",
                            domain,
                            patterns: [floorPattern(domain)],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: writeScript,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveSucceeded();
            expect(ws.read("out/floor.txt")).toBe("ran");
        });
    }

    // A *coarsely* floored domain (Node's all-or-nothing `--allow-child-process`
    // for a specific program) still warns under the default stance: the kernel
    // floor is a superset of the requested program, narrowed only by the
    // bypassable shim. The diagnostic explains the superset rather than a total
    // absence of a floor.
    for (const [rt, domain] of COARSE_FLOORED) {
        it(`${rt}: warns that governed ${domain} is only coarsely floored (still runs)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    capabilities: [
                        {
                            access: "allow",
                            domain,
                            patterns: [floorPattern(domain)],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: writeScript,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveSucceeded();
            expect(ws.read("out/floor.txt")).toBe("ran");
            expect(result).toOutputContaining(FLOOR_WARNING);
            // The coarse-floor diagnostic names the superset, not "enforced only".
            expect(result).toOutputContaining("could not be represented");
        });

        it(`${rt}: require-floor refuses when governed ${domain} is only coarsely floored`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: rt,
                    strictness: "require-floor",
                    capabilities: [
                        {
                            access: "allow",
                            domain,
                            patterns: [floorPattern(domain)],
                        },
                        {
                            access: "allow",
                            domain: "fs.write",
                            patterns: ["@project/**"],
                        },
                    ],
                    script: writeScript,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveFailed();
            expect(ws.exists("out/floor.txt")).toBe(false);
            expect(result).toHaveStderrContaining(
                "un-bypassable enforcement floor",
            );
            expect(result).toHaveStderrContaining(domain);
        });
    }
});
