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
import { describe, expect, it } from "vitest";
import {
    makeWorkspace,
    runOmni,
    skipUnlessRemoteReachable,
    type Workspace,
    type WorkspaceSpec,
} from "@/harness";

type Runtime = "node" | "bun" | "deno";
const RUNTIMES: readonly Runtime[] = ["node", "bun", "deno"];

// Runtimes whose *allow* network path is asserted end to end. All three confine
// `net` and complete an allowed request: Deno/Node lower it into `--allow-net`
// (Node needs v24+ for network permissions; older Node is refused with a clear
// message before launch) and Bun relies on the shim. The positive path is
// asserted two ways: a deterministic loopback request (an in-script HTTP server
// on 127.0.0.1, needs no external network) and a network-gated remote request
// (via `skipUnlessRemoteReachable`).
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
        it(`${rt}: permits an allowed host over loopback (no external network)`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }

            // Deterministic analog of the remote test below: the script starts
            // its own HTTP server on 127.0.0.1 and fetches it through the
            // enforced `ctx.sys.net.http.fetch`. The `net` policy allows
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
        });

        it(`${rt}: permits a host in the net allow-list`, async (ctx) => {
            if (!runtimeAvailable(rt)) {
                ctx.skip();
                return;
            }
            await skipUnlessRemoteReachable(ctx);

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
                        const res = await ctx.sys.net.http.fetch("https://github.com/");
                        await ctx.sys.fs.writeStringToFile("net-status.txt", String(res.status));
                    }`,
                }),
            );

            const result = await runCapgen(ws);

            expect(result).toHaveSucceeded();
            expect(ws.read("out/net-status.txt")).toMatch(/^\d{3}$/);
        });
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
// `net`/`process` have a floor on Deno (`--allow-net`/`--allow-run`) and Node
// (coarse but real `--allow-net`/`--allow-child-process`), but NONE on Bun
// (no permission model), so only Bun warns. This is platform-independent (no OS
// sandbox covers net/process anywhere). Pinned to omni_capability_enforcement
// `FloorGap` / plan.rs and `crates/omni_generator/src/script_runner.rs`.
const FLOOR_WARNING = "un-bypassable floor";
const NET_PROCESS_FLOOR_RUNTIMES: readonly Runtime[] = ["deno", "node"];

describe("+generator @e2e (capabilities: enforcement floor)", {
    tags: ["generator"],
    timeout: 60_000,
}, () => {
    // A script that only writes a file; the floor warning is emitted at plan
    // time, independent of whether the governed domain is actually exercised.
    const writeScript = `export default async function (ctx) {
        await ctx.sys.fs.writeStringToFile("floor.txt", "ran");
    }`;

    for (const domain of ["net", "process"] as const) {
        const pattern = domain === "net" ? "127.0.0.1:*" : "git";
        it(`bun: warns that governed ${domain} has no un-bypassable floor (still runs)`, async (ctx) => {
            if (!runtimeAvailable("bun")) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: "bun",
                    capabilities: [
                        { access: "allow", domain, patterns: [pattern] },
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

    for (const rt of NET_PROCESS_FLOOR_RUNTIMES) {
        it(`${rt}: governed net is floored by launch flags (no floor warning)`, async (ctx) => {
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
                            patterns: ["127.0.0.1:443"],
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
    // nothing is written. On Bun net/process have no un-bypassable floor on any
    // platform, so the refusal is deterministic there.
    for (const domain of ["net", "process"] as const) {
        const pattern = domain === "net" ? "127.0.0.1:*" : "git";
        it(`bun: require-floor refuses when governed ${domain} has no floor`, async (ctx) => {
            if (!runtimeAvailable("bun")) {
                ctx.skip();
                return;
            }
            const ws = makeWorkspace(
                capGeneratorSpec({
                    runtime: "bun",
                    strictness: "require-floor",
                    capabilities: [
                        { access: "allow", domain, patterns: [pattern] },
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

    // The stronger stance is a no-op when every governed domain is already
    // floored: Deno/Node lower `net` into launch flags, so require-floor still
    // proceeds and writes.
    for (const rt of NET_PROCESS_FLOOR_RUNTIMES) {
        it(`${rt}: require-floor still runs when net is floored by launch flags`, async (ctx) => {
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
                            domain: "net",
                            patterns: ["127.0.0.1:443"],
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
});
