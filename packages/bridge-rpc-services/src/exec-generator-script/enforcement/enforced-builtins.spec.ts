import { createRequire } from "node:module";
import { afterEach, describe, expect, test, vi } from "vitest";
import { CapabilityPolicy } from "./capability-policy";
import {
    installBuiltinModuleEnforcement,
    netTargetFromConnectArgs,
    programFromArg,
    programFromCommandLine,
    programFromSpawnOptions,
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

describe("programFromArg / programFromCommandLine", () => {
    test("program-first extraction", () => {
        expect(programFromArg("git")).toBe("git");
        expect(programFromArg("")).toBeNull();
        expect(programFromArg(123)).toBeNull();
    });

    test("shell command line yields its first token", () => {
        expect(programFromCommandLine("git status --short")).toBe("git");
        expect(programFromCommandLine("   rm   -rf /   ")).toBe("rm");
        expect(programFromCommandLine("")).toBeNull();
        expect(programFromCommandLine(42)).toBeNull();
    });
});

describe("programFromSpawnOptions (ChildProcess.prototype.spawn payload)", () => {
    test("a direct spawn resolves to its executable", () => {
        expect(
            programFromSpawnOptions({ file: "git", args: ["git", "status"] }),
        ).toBe("git");
    });

    test("a shell invocation resolves to the command line's first token", () => {
        expect(
            programFromSpawnOptions({
                file: "/bin/sh",
                args: ["/bin/sh", "-c", "rm -rf /tmp"],
                shell: true,
            }),
        ).toBe("rm");
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

    test("exec authorizes the command line's first token", () => {
        const exec = stub("exec");
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy);

        expect(() =>
            (cp.exec as (...a: unknown[]) => unknown)("rm -rf /"),
        ).toThrow(ProcessPolicyError);
        expect(exec).not.toHaveBeenCalled();
        (cp.exec as (...a: unknown[]) => unknown)("git status");
        expect(exec).toHaveBeenCalledOnce();
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

    test("authorizes the inner command of a shell invocation", () => {
        const protoSpawn = stubProtoSpawn();
        const policy = CapabilityPolicy.parse(
            enforceJson({ process: { allow: ["git"] } }),
        );
        installBuiltinModuleEnforcement(policy);

        const child = Object.create(cpProto.ChildProcess.prototype) as {
            spawn: (...a: unknown[]) => unknown;
        };
        expect(() =>
            child.spawn({
                file: "/bin/sh",
                args: ["/bin/sh", "-c", "rm -rf /"],
                shell: true,
            }),
        ).toThrow(ProcessPolicyError);
        expect(protoSpawn).not.toHaveBeenCalled();

        child.spawn({
            file: "/bin/sh",
            args: ["/bin/sh", "-c", "git status"],
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
