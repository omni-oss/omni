import { describe, expect, test } from "vitest";
import {
    ENV_INJECTION_DENYLIST,
    isEnvInjectionVector,
    isPathKey,
    scrubChildEnv,
} from "./enforced-env";

describe("ENV_INJECTION_DENYLIST", () => {
    test("mirrors the Rust ENV_INJECTION_DENYLIST set", () => {
        // Kept in lock-step with `crates/bridge_rpc_runner/src/runner.rs`; if the
        // Rust list changes, this must change with it (and vice versa).
        expect([...ENV_INJECTION_DENYLIST].sort()).toEqual(
            [
                "DYLD_FALLBACK_FRAMEWORK_PATH",
                "DYLD_FALLBACK_LIBRARY_PATH",
                "DYLD_FRAMEWORK_PATH",
                "DYLD_INSERT_LIBRARIES",
                "DYLD_LIBRARY_PATH",
                "LD_AUDIT",
                "LD_LIBRARY_PATH",
                "LD_PRELOAD",
                "NODE_OPTIONS",
            ].sort(),
        );
    });
});

describe("isEnvInjectionVector", () => {
    test("matches denylisted vars case-insensitively", () => {
        expect(isEnvInjectionVector("LD_PRELOAD")).toBe(true);
        expect(isEnvInjectionVector("ld_preload")).toBe(true);
        expect(isEnvInjectionVector("Dyld_Insert_Libraries")).toBe(true);
        expect(isEnvInjectionVector("NODE_OPTIONS")).toBe(true);
    });

    test("passes ordinary vars", () => {
        expect(isEnvInjectionVector("HOME")).toBe(false);
        expect(isEnvInjectionVector("MY_TOKEN")).toBe(false);
    });
});

describe("isPathKey", () => {
    test("matches PATH case-insensitively", () => {
        expect(isPathKey("PATH")).toBe(true);
        expect(isPathKey("Path")).toBe(true);
        expect(isPathKey("path")).toBe(true);
        expect(isPathKey("PATHEXT")).toBe(false);
    });
});

describe("scrubChildEnv", () => {
    test("drops every code-injection vector, keeps benign vars", () => {
        const scrubbed = scrubChildEnv({
            HOME: "/home/user",
            MY_TOKEN: "secret",
            LD_PRELOAD: "/tmp/evil.so",
            ld_library_path: "/tmp/evil",
            NODE_OPTIONS: "--require=/tmp/x.js",
        });
        expect(scrubbed).toEqual({ HOME: "/home/user", MY_TOKEN: "secret" });
    });

    test("drops a caller PATH and pins the trusted one when given", () => {
        const scrubbed = scrubChildEnv(
            { PATH: "/tmp/evil:/usr/bin", MY: "1" },
            "/usr/bin:/bin",
        );
        expect(scrubbed.PATH).toBe("/usr/bin:/bin");
        expect(scrubbed.MY).toBe("1");
    });

    test("drops a caller PATH entirely when no trusted path is given", () => {
        const scrubbed = scrubChildEnv({ PATH: "/tmp/evil", MY: "1" });
        expect(scrubbed).toEqual({ MY: "1" });
    });

    test("skips undefined values", () => {
        const scrubbed = scrubChildEnv({ A: "1", B: undefined });
        expect(scrubbed).toEqual({ A: "1" });
    });
});
