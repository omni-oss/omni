import { describe, expect, test } from "vitest";
import { CapabilityPolicy } from "./capability-policy";
import { confinedEnv, createEnforcedSpawn } from "./enforced-process";

describe("confinedEnv", () => {
    test("drops code-injection vectors supplied as overrides", () => {
        const env = confinedEnv({
            LD_PRELOAD: "/tmp/evil.so",
            NODE_OPTIONS: "--require=/tmp/x.js",
            dyld_insert_libraries: "/tmp/evil.dylib",
            MY_TOKEN: "keep-me",
        });
        expect(env.LD_PRELOAD).toBeUndefined();
        expect(env.NODE_OPTIONS).toBeUndefined();
        expect(env.dyld_insert_libraries).toBeUndefined();
        expect(env.MY_TOKEN).toBe("keep-me");
    });

    test("refuses a PATH override, keeping the trusted inherited value", () => {
        const trusted = process.env.PATH;
        const env = confinedEnv({ PATH: "/tmp/evil" });
        // The caller cannot redirect program resolution: PATH stays whatever the
        // trusted parent had (or absent if the parent had none), never the
        // attacker's value.
        expect(env.PATH).toBe(trusted);
        expect(env.PATH).not.toBe("/tmp/evil");
    });
});

describe("createEnforcedSpawn (end-to-end env neutralization)", () => {
    test.skipIf(process.platform === "win32")(
        "a confined spawn strips LD_PRELOAD and cannot hijack PATH",
        async () => {
            const spawn = createEnforcedSpawn(CapabilityPolicy.empty(), () =>
                process.cwd(),
            );
            const result = await spawn(process.execPath, {
                args: [
                    "-e",
                    "process.stdout.write(JSON.stringify({" +
                        "ld: process.env.LD_PRELOAD ?? null," +
                        "path: process.env.PATH ?? null," +
                        "my: process.env.MY_TOKEN ?? null}))",
                ],
                env: {
                    LD_PRELOAD: "/tmp/evil.so",
                    PATH: "/tmp/evil",
                    MY_TOKEN: "kept",
                },
            });
            expect(result.code).toBe(0);
            const seen = JSON.parse(result.stdout) as {
                ld: string | null;
                path: string | null;
                my: string | null;
            };
            // The injection vector never reached the child...
            expect(seen.ld).toBeNull();
            // ...the hijack PATH was refused (the child sees the trusted one)...
            expect(seen.path).not.toBe("/tmp/evil");
            expect(seen.path).toBe(process.env.PATH ?? null);
            // ...but a benign override is preserved.
            expect(seen.my).toBe("kept");
        },
    );
});
