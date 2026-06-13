/**
 * `omni init` - bootstrap a workspace from a git repository's primary
 * generator. Pinned to `crates/omni_cli_core/src/commands/init.rs`.
 *
 * `init` clones `--git <url>` into a temp dir, finds the repo's single root
 * generator (`omni_generator::discover_one_in_dir`), and runs it into the
 * output dir (`-o`, default cwd). It forwards `-v`/`--use-defaults` to that
 * run. The happy-path tests clone a real public repo over HTTPS, so they are
 * gated on network reachability via `skipUnlessRemoteReachable`.
 *
 * Runs stay non-interactive by always supplying `-v workspace_name=...` so the
 * generator's single text prompt never blocks. The repo's `add` action writes
 * `workspace.omni.yaml` from `workspace.omni.yaml.tpl` (its sibling `**`
 * sources are dot-files, which the globs skip), giving a deterministic output
 * file to assert on.
 */

import { mkdirSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
    makeWorkspace,
    runOmni,
    skipUnlessRemoteReachable,
    skipUnlessSshReachable,
    workspaceMinimalRepo,
} from "@/harness";

const repo = workspaceMinimalRepo;

// Cloning over the network can be slow on cold CI runners; give clone-backed
// tests more headroom than the default 30s.
const CLONE_TIMEOUT_MS = 60_000;

describe("+init @e2e (clone + run primary generator)", () => {
    it(
        "`--git <url>` clones the repo and runs its primary generator into cwd",
        async (ctx) => {
            await skipUnlessRemoteReachable(ctx);

            // A fresh empty dir as cwd: `init` writes the new workspace here,
            // and an existing `workspace.omni.yaml` would trigger an overwrite
            // prompt that hangs under a non-interactive run.
            const ws = makeWorkspace();
            const dest = ws.path("init-here");
            mkdirSync(dest, { recursive: true });

            const result = await runOmni(
                [
                    "init",
                    "--git",
                    repo.https,
                    "-v",
                    `${repo.promptName}=cwd-ws`,
                    "--use-defaults",
                ],
                { cwd: dest, timeout: CLONE_TIMEOUT_MS },
            );

            expect(result).toHaveSucceeded();
            const workspaceFile = ws.read("init-here/workspace.omni.yaml");
            expect(workspaceFile).toContain("name: cwd-ws");
        },
        CLONE_TIMEOUT_MS,
    );

    it(
        "`-o/--output <dir>` initializes into the given directory",
        async (ctx) => {
            await skipUnlessRemoteReachable(ctx);

            const ws = makeWorkspace();

            const result = await runOmni(
                [
                    "init",
                    "--git",
                    repo.https,
                    "-o",
                    "generated",
                    "-v",
                    `${repo.promptName}=out-ws`,
                    "--use-defaults",
                ],
                { cwd: ws.cwd, timeout: CLONE_TIMEOUT_MS },
            );

            expect(result).toHaveSucceeded();
            expect(ws.exists("generated/workspace.omni.yaml")).toBe(true);
            expect(ws.read("generated/workspace.omni.yaml")).toContain(
                "name: out-ws",
            );
        },
        CLONE_TIMEOUT_MS,
    );

    it(
        "forwards `-v/--value` to the generator run so prompts are prefilled",
        async (ctx) => {
            await skipUnlessRemoteReachable(ctx);

            const ws = makeWorkspace();
            const customName = "forwarded-name";

            const result = await runOmni(
                [
                    "init",
                    "--git",
                    repo.https,
                    "-o",
                    "fwd",
                    "-v",
                    `${repo.promptName}=${customName}`,
                    "--use-defaults",
                ],
                { cwd: ws.cwd, timeout: CLONE_TIMEOUT_MS },
            );

            expect(result).toHaveSucceeded();
            // The `-v` value reaching the generator is proven by it being baked
            // into the rendered `workspace.omni.yaml`.
            expect(ws.read("fwd/workspace.omni.yaml")).toContain(
                `name: ${customName}`,
            );
        },
        CLONE_TIMEOUT_MS,
    );
});

describe("+init @e2e @scm (SSH remote)", () => {
    it(
        "`--git <scp-url>` clones over SSH using the machine's keys",
        async (ctx) => {
            // gix shells out to the system `ssh`, so the SCP-style URL works
            // with existing keys/ssh-agent. Gated on SSH access so it runs on
            // dev boxes and skips on CI/contributors without an authorized key.
            await skipUnlessSshReachable(ctx);

            const ws = makeWorkspace();
            const dest = ws.path("init-ssh");
            mkdirSync(dest, { recursive: true });

            const result = await runOmni(
                [
                    "init",
                    "--git",
                    repo.ssh,
                    "-v",
                    `${repo.promptName}=ssh-ws`,
                    "--use-defaults",
                ],
                { cwd: dest, timeout: CLONE_TIMEOUT_MS },
            );

            expect(result).toHaveSucceeded();
            expect(ws.read("init-ssh/workspace.omni.yaml")).toContain(
                "name: ssh-ws",
            );
        },
        CLONE_TIMEOUT_MS,
    );
});

describe("+init @e2e @exitcode (error paths)", () => {
    it("with no `--git` logs an error and exits without action", async () => {
        const ws = makeWorkspace();

        const result = await runOmni(["init", "-o", "nope"], {
            cwd: ws.cwd,
        });

        // `init` returns Ok after logging, so the exit code stays 0.
        expect(result).toHaveExitCode(0);
        expect(result).toOutputContaining("No source provided");
        expect(ws.exists("nope/workspace.omni.yaml")).toBe(false);
    });

    it(
        "a repo with no primary generator logs a clear error",
        async (ctx) => {
            await skipUnlessRemoteReachable(ctx);

            // octocat/Hello-World is a stable, tiny public repo that has no
            // `generator.omni.yaml` at its root, so discovery finds none.
            const ws = makeWorkspace();

            const result = await runOmni(
                [
                    "init",
                    "--git",
                    "https://github.com/octocat/Hello-World.git",
                    "-o",
                    "empty",
                    "-v",
                    `${repo.promptName}=x`,
                    "--use-defaults",
                ],
                { cwd: ws.cwd, timeout: CLONE_TIMEOUT_MS },
            );

            expect(result).toHaveExitCode(0);
            expect(result).toOutputContaining("No primary generator is found");
            expect(ws.exists("empty/workspace.omni.yaml")).toBe(false);
        },
        CLONE_TIMEOUT_MS,
    );

    it("an invalid/unreachable git URL surfaces a clone error", async () => {
        // `.invalid` is reserved (RFC 6761) and never resolves, so this fails
        // fast at the transport layer with no credential prompt - no network
        // gating needed and no risk of hanging on auth.
        const ws = makeWorkspace();

        const result = await runOmni(
            [
                "init",
                "--git",
                "https://github.invalid/nope/nope.git",
                "-o",
                "broken",
                "-v",
                `${repo.promptName}=x`,
                "--use-defaults",
            ],
            { cwd: ws.cwd, timeout: CLONE_TIMEOUT_MS },
        );

        expect(result).toHaveExitCode(1);
        expect(result.failed).toBe(true);
        expect(ws.exists("broken/workspace.omni.yaml")).toBe(false);
    });
});
