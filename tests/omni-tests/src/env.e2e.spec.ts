/**
 * `omni env` - resolves and prints environment variables for the workspace.
 * Pinned to `crates/omni_cli_core/src/commands/env.rs` (and the env resolution
 * in `crates/omni_cli_core/src/context/mod.rs`).
 *
 * `env get` uses `print!` (no trailing newline); a missing key emits a warn-level
 * log (on stdout) and prints no value. We pass `-l off` when we need a clean
 * value with no log noise.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni } from "@/harness";

/** Workspace whose root `.env` defines the given vars. */
function envWorkspace(vars: Record<string, string>) {
    const dotenv = `${Object.entries(vars)
        .map(([k, v]) => `${k}=${v}`)
        .join("\n")}\n`;
    return makeWorkspace({
        workspace: { projects: ["**"] },
        files: { ".env": dotenv },
    });
}

describe("+env @env (resolution output)", () => {
    it("`env all` prints KEY=VALUE for every resolved var", async () => {
        const ws = envWorkspace({ FOO: "foo", BAR: "bar" });

        const result = await runOmni(["-l", "off", "env", "all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.out).toContain("FOO=foo");
        expect(result.out).toContain("BAR=bar");
    });

    it("`env get <key>` prints the value with no trailing newline", async () => {
        const ws = envWorkspace({ FOO: "foo" });

        const result = await runOmni(["-l", "off", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.stdout).toBe("foo");
    });

    it("`env get <missing>` warns and prints no value", async () => {
        const ws = envWorkspace({ FOO: "foo" });

        // At the default level the warning is emitted...
        const warned = await runOmni(["env", "get", "MISSING"], {
            cwd: ws.cwd,
        });
        expect(warned).toHaveSucceeded();
        expect(warned).toOutputContaining("does not exists");

        // ...and with logs off there is no value (or anything) on stdout.
        const silent = await runOmni(["-l", "off", "env", "get", "MISSING"], {
            cwd: ws.cwd,
        });
        expect(silent.stdout).toBe("");
    });
});

describe("+env @env (merging & flags)", () => {
    it("merges workspace env files with per-directory env files", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env": "ROOTVAR=rootval\n",
                "sub/.env": "SUBVAR=subval\n",
            },
        });

        const result = await runOmni(["-l", "off", "env", "all"], {
            cwd: ws.path("sub"),
        });

        expect(result).toHaveSucceeded();
        expect(result.out).toContain("ROOTVAR=rootval");
        expect(result.out).toContain("SUBVAR=subval");
    });

    it("--inherit-env-vars surfaces parent process vars in `env all`", async () => {
        const ws = envWorkspace({ FOO: "foo" });

        const inherited = await runOmni(["-l", "off", "-i", "env", "all"], {
            cwd: ws.cwd,
            env: { PARENT_MARKER: "hello" },
        });
        const isolated = await runOmni(["-l", "off", "env", "all"], {
            cwd: ws.cwd,
            env: { PARENT_MARKER: "hello" },
        });

        expect(inherited.out).toContain("PARENT_MARKER=hello");
        expect(isolated.out).not.toContain("PARENT_MARKER=hello");
    });

    it("substitutes {ENV} in env file names against the --env value", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".env.staging": "STG=stgval\n" },
        });

        const result = await runOmni(
            [
                "-l",
                "off",
                "-e",
                ".env.{ENV}",
                "--env",
                "staging",
                "env",
                "get",
                "STG",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).toBe("stgval");
    });
});

describe("+env @env (precedence & combined flags)", () => {
    it("`env all -i --env <env> -e .env.{ENV}` combines inherit, env selection, and {ENV}", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".env.production": "PRODVAR=prodval\n" },
        });

        const result = await runOmni(
            [
                "-l",
                "off",
                "-i",
                "--env",
                "production",
                "-e",
                ".env.{ENV}",
                "env",
                "all",
            ],
            { cwd: ws.cwd, env: { PARENT_MARKER: "hello" } },
        );

        expect(result).toHaveSucceeded();
        // `{ENV}` -> production selects the production file...
        expect(result.out).toContain("PRODVAR=prodval");
        // ...and `-i` surfaces the parent process var alongside it.
        expect(result.out).toContain("PARENT_MARKER=hello");
    });

    it("`env get <key> -i`: an env file overrides the inherited parent value", async () => {
        // Both define SHARED. The loader seeds inherited parent vars first and
        // then extends with env-file values (see `extra_envs` + `env.extend` in
        // `crates/env_loader/src/lib.rs`), so the file wins.
        const ws = envWorkspace({ SHARED: "fromfile" });

        const result = await runOmni(
            ["-l", "off", "-i", "env", "get", "SHARED"],
            { cwd: ws.cwd, env: { SHARED: "fromparent" } },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).toBe("fromfile");
    });

    it("loads every file when -e/--env-file is repeated", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env.one": "ONE=oneval\n",
                ".env.two": "TWO=twoval\n",
                ".env.three": "THREE=threeval\n",
            },
        });

        const result = await runOmni(
            [
                "-l",
                "off",
                "-e",
                ".env.one",
                "--env-file",
                ".env.two",
                "-e",
                ".env.three",
                "env",
                "all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.out).toContain("ONE=oneval");
        expect(result.out).toContain("TWO=twoval");
        expect(result.out).toContain("THREE=threeval");
    });

    it("-r/--env-root-dir-marker stops discovery at the custom marker", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env": "ROOTVAR=rootval\n",
                "sub/mark.txt": "",
                "sub/.env": "SUBVAR=subval\n",
            },
        });

        const result = await runOmni(
            ["-l", "off", "-r", "mark.txt", "env", "all"],
            { cwd: ws.path("sub") },
        );

        expect(result).toHaveSucceeded();
        expect(result.out).toContain("SUBVAR=subval");
        // The marker in `sub` halts the climb before reaching the root `.env`.
        expect(result.out).not.toContain("ROOTVAR=rootval");
    });
});
