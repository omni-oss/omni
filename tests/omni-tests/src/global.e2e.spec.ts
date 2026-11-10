/**
 * Top-level CLI surface: version, help, dispatch, and the global flags shared by
 * every subcommand (logging, traces, file traces, and env resolution).
 *
 * These pin the behavior wired up in `crates/omni_cli_core/src/commands/mod.rs`
 * (the clap `Cli`/`CliArgs` definitions) and `cli/omni/src/lib.rs` (trace-file
 * resolution + dispatch). Where a flag's observable effect is a log/trace, we
 * drive a tiny workspace and a known command (`env get`, `run`) so the assertion
 * is deterministic.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni } from "@/harness";

const SUBCOMMANDS = [
    "env",
    "exec",
    "config",
    "completion",
    "run",
    "hash",
    "declspec",
    "cache",
    "generator",
    "init",
    "project",
];

/** A workspace whose `.env` defines the given vars, for env/log assertions. */
function envWorkspace(env: Record<string, string> = { FOO: "bar" }) {
    const dotenv = Object.entries(env)
        .map(([k, v]) => `${k}=${v}`)
        .join("\n");
    return makeWorkspace({
        workspace: { projects: ["**"] },
        files: { ".env": `${dotenv}\n` },
    });
}

describe("+global @cli (version & help)", () => {
    it("`omni --version` prints the package version", async () => {
        const result = await runOmni(["--version"]);

        expect(result).toHaveSucceeded();
        expect(result).toMatchOutput(/^omni \d+\.\d+\.\d+/);
    });

    it("`omni --help` lists every subcommand", async () => {
        const result = await runOmni(["--help"]);

        expect(result).toHaveSucceeded();
        for (const command of SUBCOMMANDS) {
            expect(result).toOutputContaining(command);
        }
    });

    it("`--version` propagates to subcommands", async () => {
        const result = await runOmni(["run", "--version"]);

        expect(result).toHaveSucceeded();
        expect(result).toMatchOutput(/\d+\.\d+\.\d+/);
    });

    it("`gen` is an alias for `generator`", async () => {
        const alias = await runOmni(["gen", "--help"]);
        const full = await runOmni(["generator", "--help"]);

        expect(alias).toHaveSucceeded();
        expect(full).toHaveSucceeded();
        expect(alias.out).toBe(full.out);
        expect(alias).toOutputContaining("Code generation");
    });
});

describe("+global @exitcode (dispatch errors)", () => {
    it("an unknown subcommand fails with a usage error", async () => {
        const result = await runOmni(["definitely-not-a-subcommand"]);

        expect(result).toHaveFailed();
        expect(result.exitCode).not.toBe(0);
        expect(result).toHaveStderrContaining("unrecognized subcommand");
        expect(result).toHaveStderrContaining("Usage:");
    });
});

describe("+global @cli (notices and -q)", () => {
    const MISSING_WARN = "environmental variable does not exists";

    it("reports a notice on stderr with its code, without changing the exit status", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["env", "get", "MISSING_VAR"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("warning[env.var_missing]");
        expect(result).toHaveStderrContaining(MISSING_WARN);
        expect(result).toHaveStderrContaining(
            "omni code explain env.var_missing",
        );
        // stdout carries results only.
        expect(result.stdout).toBe("");
    });

    it("-q/--quiet hides warn notices", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["-q", "env", "get", "MISSING"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.err).not.toContain(MISSING_WARN);
    });

    it("honors OMNI_QUIET", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["env", "get", "MISSING"], {
            cwd: ws.cwd,
            env: { OMNI_QUIET: "true" },
        });

        expect(result).toHaveSucceeded();
        expect(result.err).not.toContain(MISSING_WARN);
    });

    it.each([
        ["-l", "off"],
        ["--stdout-log-level", "off"],
        ["--stderr-log-level", "warn"],
        ["-t"],
        ["--stdout-show-traces"],
        ["--stderr-show-traces"],
        ["--file-trace-output", "x.log"],
        ["-f", "info"],
    ])("the removed flag `%s` is an unknown argument", async (...flag) => {
        const ws = envWorkspace();

        const result = await runOmni([...flag, "env", "get", "FOO"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("unexpected argument");
    });
});

describe("+global @cli (--trace)", () => {
    // `tracing_initialized` is a native `trace!` event, so it only appears when
    // developer traces are enabled.
    const NATIVE_TRACE = "tracing_initialized";

    it("is off by default", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["env", "get", "FOO"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result.err).not.toContain(NATIVE_TRACE);
    });

    it("--trace <filter> writes traces to stderr, never to stdout", async () => {
        const ws = envWorkspace();

        const result = await runOmni(
            ["--trace", "trace", "env", "get", "FOO"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining(NATIVE_TRACE);
        expect(result.stdout).toBe("bar");
    });

    it("the filter selects what is shown", async () => {
        const ws = envWorkspace();

        const error = await runOmni(["--trace", "error", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });
        const all = await runOmni(["--trace", "trace", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });

        expect(error.err).not.toContain(NATIVE_TRACE);
        expect(all).toHaveStderrContaining(NATIVE_TRACE);
    });

    it("honors OMNI_TRACE, and an explicit flag overrides it", async () => {
        const ws = envWorkspace();

        const fromEnv = await runOmni(["env", "get", "FOO"], {
            cwd: ws.cwd,
            env: { OMNI_TRACE: "trace" },
        });
        const overridden = await runOmni(
            ["--trace", "error", "env", "get", "FOO"],
            {
                cwd: ws.cwd,
                env: { OMNI_TRACE: "trace" },
            },
        );

        expect(fromEnv).toHaveStderrContaining(NATIVE_TRACE);
        expect(overridden.err).not.toContain(NATIVE_TRACE);
    });

    it("an invalid filter is reported", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["--trace", "=,=", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("--trace");
    });

    it("no tracing layer writes to stdout, at any setting", async () => {
        const ws = envWorkspace();

        const result = await runOmni(
            [
                "--trace",
                "trace",
                "--trace-file-level",
                "trace",
                "env",
                "get",
                "FOO",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).toBe("bar");
    });
});

describe("+global @cli (file traces)", () => {
    it("--trace-file + --trace-file-level write JSON traces to the given file", async () => {
        const ws = envWorkspace();

        // Query a missing var so a warn-level event is actually written to the
        // file (an existing var logs nothing at the `info` file level).
        const result = await runOmni(
            [
                "--trace-file-level",
                "info",
                "--trace-file",
                "./trace.log",
                "env",
                "get",
                "MISSING",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("trace.log")).toBe(true);
        expect(ws.read("trace.log")).toContain('"level"');
    });

    it("defaults the trace file to ./.omni/trace/omni.log", async () => {
        const ws = envWorkspace();

        const result = await runOmni(
            ["--trace-file-level", "info", "env", "get", "FOO"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists(".omni/trace/omni.log")).toBe(true);
    });

    it("resolves a relative trace path against the workspace root", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { "sub/deep/.keep": "" },
        });

        const result = await runOmni(
            ["--trace-file-level", "info", "env", "get", "FOO"],
            {
                cwd: ws.path("sub", "deep"),
            },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists(".omni/trace/omni.log")).toBe(true);
        expect(ws.exists("sub/deep/.omni/trace/omni.log")).toBe(false);
    });
});

describe("+global @env (env resolution)", () => {
    it("-r/--env-root-dir-marker overrides where the env search stops", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env": "ROOT=rootval\n",
                "sub/.env": "SUB=subval\n",
                "sub/mark.txt": "",
            },
        });
        const cwd = ws.path("sub");

        // Default marker (workspace.omni.yaml) lets the search climb to the root.
        const def = await runOmni(["env", "get", "ROOT"], { cwd });
        expect(def).toOutputContaining("rootval");

        // A marker present in `sub` stops the climb there, hiding the root `.env`.
        const overridden = await runOmni(
            ["-r", "mark.txt", "env", "get", "ROOT"],
            { cwd },
        );
        expect(overridden.out).not.toContain("rootval");
        const stillLocal = await runOmni(
            ["-r", "mark.txt", "env", "get", "SUB"],
            { cwd },
        );
        expect(stillLocal).toOutputContaining("subval");
    });

    it("--env / OMNI_ENV select the environment (default development)", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env.development": "PICK=dev\n",
                ".env.staging": "PICK=stg\n",
            },
        });

        const def = await runOmni(["-e", ".env.{ENV}", "env", "get", "PICK"], {
            cwd: ws.cwd,
        });
        expect(def).toOutputContaining("dev");

        const flag = await runOmni(
            ["-e", ".env.{ENV}", "--env", "staging", "env", "get", "PICK"],
            {
                cwd: ws.cwd,
            },
        );
        expect(flag).toOutputContaining("stg");

        const fromEnv = await runOmni(
            ["-e", ".env.{ENV}", "env", "get", "PICK"],
            {
                cwd: ws.cwd,
                env: { OMNI_ENV: "staging" },
            },
        );
        expect(fromEnv).toOutputContaining("stg");
    });

    it("-e/--env-file is appendable and substitutes {ENV}", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env.a": "A=aval\n",
                ".env.b": "B=bval\n",
                ".env.production": "SUBBED=prod\n",
            },
        });

        // Both repeated `-e` files are loaded.
        const a = await runOmni(
            ["-e", ".env.a", "-e", ".env.b", "env", "get", "A"],
            { cwd: ws.cwd },
        );
        const b = await runOmni(
            ["-e", ".env.a", "-e", ".env.b", "env", "get", "B"],
            { cwd: ws.cwd },
        );
        expect(a).toOutputContaining("aval");
        expect(b).toOutputContaining("bval");

        // `{ENV}` in the filename is replaced by the selected environment.
        const subbed = await runOmni(
            ["-e", ".env.{ENV}", "--env", "production", "env", "get", "SUBBED"],
            { cwd: ws.cwd },
        );
        expect(subbed).toOutputContaining("prod");
    });

    it("-i/--inherit-env-vars exposes the parent process env", async () => {
        const ws = makeWorkspace({ workspace: { projects: ["**"] } });

        const inherited = await runOmni(["-i", "env", "get", "PARENT_MARKER"], {
            cwd: ws.cwd,
            env: { PARENT_MARKER: "hello" },
        });
        const isolated = await runOmni(["env", "get", "PARENT_MARKER"], {
            cwd: ws.cwd,
            env: { PARENT_MARKER: "hello" },
        });

        expect(inherited).toOutputContaining("hello");
        expect(isolated.out).not.toContain("hello");
    });
});

describe("+global @cli (file traces at a requested level)", () => {
    it("`--trace-file <path>` + `--trace-file-level debug` writes debug traces there", async () => {
        const ws = envWorkspace();

        const result = await runOmni(
            [
                "--trace-file-level",
                "debug",
                "--trace-file",
                "./debug-trace.log",
                "env",
                "get",
                "FOO",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("debug-trace.log")).toBe(true);
        const contents = ws.read("debug-trace.log");
        // JSON trace entries carry a level; `-f debug` lets DEBUG events through.
        expect(contents).toContain('"level"');
        expect(contents).toContain('"DEBUG"');
    });
});

describe("+global @cli (explicit flags override env-var forms)", () => {
    it("`--env <env>` overrides OMNI_ENV", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env.development": "PICK=dev\n",
                ".env.staging": "PICK=stg\n",
            },
        });

        // OMNI_ENV asks for staging, but `--env development` wins, so the
        // development file is selected.
        const result = await runOmni(
            [
                "-q",
                "-e",
                ".env.{ENV}",
                "--env",
                "development",
                "env",
                "get",
                "PICK",
            ],
            { cwd: ws.cwd, env: { OMNI_ENV: "staging" } },
        );

        expect(result.stdout).toBe("dev");
    });

    it("`--trace-file-level` overrides OMNI_TRACE_FILE_LEVEL", async () => {
        const ws = envWorkspace();

        // The env var disables file traces; the explicit flag turns them back
        // on, so the trace file is created.
        const result = await runOmni(
            [
                "-q",
                "--trace-file-level",
                "info",
                "--trace-file",
                "./override.log",
                "env",
                "get",
                "MISSING",
            ],
            { cwd: ws.cwd, env: { OMNI_TRACE_FILE_LEVEL: "off" } },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("override.log")).toBe(true);
    });
});

describe("+global @env (global flags take effect before the subcommand)", () => {
    it("`-r` + `--env` + `-e` all apply before `env get` runs", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                // Root copy that must be ignored once the marker stops the climb.
                ".env.production": "PICK=prodroot\n",
                "sub/mark.txt": "",
                "sub/.env.production": "PICK=prodsub\n",
            },
        });
        const cwd = ws.path("sub");

        const result = await runOmni(
            [
                "-q",
                "-r",
                "mark.txt",
                "--env",
                "production",
                "-e",
                ".env.{ENV}",
                "env",
                "get",
                "PICK",
            ],
            { cwd },
        );

        // `-r` stops discovery at `sub`, `--env` selects production, and `-e`
        // substitutes `{ENV}` - so only `sub/.env.production` is loaded.
        expect(result.stdout).toBe("prodsub");
        expect(result.out).not.toContain("prodroot");
    });
});
