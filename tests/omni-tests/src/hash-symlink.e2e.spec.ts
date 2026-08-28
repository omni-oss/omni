/**
 * `omni hash` follows symlinks when hashing a project's inputs, but only for
 * targets that stay inside the workspace root. Content reached through an
 * in-workspace symlinked directory must participate in the hash; content behind
 * a symlink whose real target escapes the workspace must not, so a cache key can
 * never depend on machine-external state.
 *
 * Pinned to the collector's walk in `crates/omni_collector/src/collector.rs`.
 */

import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, onTestFinished } from "vitest";
import { makeWorkspace, runOmni } from "@/harness";

const RAW_HASH = /^[A-Za-z0-9]{40,}$/;

function symlinkFollowingProject() {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                cache: { key: { files: ["linked/**/*.txt"] } },
                tasks: { build: 'echo "build app"' },
            },
        },
    };
}

describe("+hash @e2e (symlink following, bounded to the workspace)", () => {
    it("hashes content reached through an in-workspace symlinked directory", async () => {
        const ws = makeWorkspace(symlinkFollowingProject());

        ws.write("app/real/data.txt", "original");
        symlinkSync(ws.path("app/real"), ws.path("app/linked"), "dir");

        const before = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });
        expect(before).toHaveSucceeded();
        expect(before.stdout).toMatch(RAW_HASH);

        ws.write("app/real/data.txt", "a completely different, longer body");

        const after = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });
        expect(after).toHaveSucceeded();
        expect(after.stdout).not.toBe(before.stdout);
    });

    it("excludes content behind a symlink whose target escapes the workspace", async () => {
        const outside = mkdtempSync(join(tmpdir(), "omni-e2e-ext-"));
        onTestFinished(() => rmSync(outside, { recursive: true, force: true }));
        writeFileSync(join(outside, "secret.txt"), "external-one");

        const ws = makeWorkspace(symlinkFollowingProject());
        symlinkSync(outside, ws.path("app/linked"), "dir");

        const before = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });
        expect(before).toHaveSucceeded();
        expect(before.stdout).toMatch(RAW_HASH);

        writeFileSync(
            join(outside, "secret.txt"),
            "external-two, different and longer",
        );

        const after = await runOmni(["hash", "-r", "project", "app"], {
            cwd: ws.cwd,
        });
        expect(after).toHaveSucceeded();
        expect(after.stdout).toBe(before.stdout);
    });
});
