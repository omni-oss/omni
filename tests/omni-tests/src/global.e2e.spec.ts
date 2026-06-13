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

describe("+global @cli (stdout logging)", () => {
    const MISSING_WARN = "environmental variable does not exists";

    it("emits warn-level logs to stdout at the default level", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["env", "get", "MISSING_VAR"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining(MISSING_WARN);
    });

    it("-l/--stdout-logs-level off silences stdout logs", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["-l", "off", "env", "get", "MISSING"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.out).not.toContain(MISSING_WARN);
    });

    it("honors OMNI_STDOUT_LOG_LEVEL", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["env", "get", "MISSING"], {
            cwd: ws.cwd,
            env: { OMNI_STDOUT_LOG_LEVEL: "off" },
        });

        expect(result).toHaveSucceeded();
        expect(result.out).not.toContain(MISSING_WARN);
    });
});

describe("+global @cli (stdout traces)", () => {
    // `tracing_initialized` is emitted via a native `trace!` (not the `log`
    // facade), so it only surfaces on stdout when traces are explicitly enabled.
    const NATIVE_TRACE = "tracing_initialized";

    it("-t/--stdout-show-traces surfaces native traces on stdout", async () => {
        const ws = envWorkspace();

        const withTraces = await runOmni(
            ["-l", "trace", "-t", "env", "get", "FOO"],
            { cwd: ws.cwd },
        );
        const withoutTraces = await runOmni(
            ["-l", "trace", "env", "get", "FOO"],
            { cwd: ws.cwd },
        );

        expect(withTraces).toOutputContaining(NATIVE_TRACE);
        expect(withoutTraces.out).not.toContain(NATIVE_TRACE);
    });

    it("honors OMNI_STDOUT_SHOW_TRACES", async () => {
        const ws = envWorkspace();

        const result = await runOmni(["-l", "trace", "env", "get", "FOO"], {
            cwd: ws.cwd,
            env: { OMNI_STDOUT_SHOW_TRACES: "true" },
        });

        expect(result).toOutputContaining(NATIVE_TRACE);
    });
});

describe("+global @cli (stderr logging)", () => {
    // The eyre report is always printed to stderr on failure; the error *trace*
    // (`ERROR omni:`) is what the stderr log layer adds, so we assert on that.
    const ERROR_TRACE = "ERROR omni:";

    function failingWorkspace() {
        return makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo hi" } },
            },
        });
    }

    it("--stderr-log gates the stderr log layer", async () => {
        const ws = failingWorkspace();

        const enabled = await runOmni(
            ["--stderr-log", "--stderr-show-traces", "run", "nope"],
            { cwd: ws.cwd },
        );
        const disabled = await runOmni(
            ["--stderr-show-traces", "run", "nope"],
            { cwd: ws.cwd },
        );

        expect(enabled).toHaveFailed();
        expect(enabled).toHaveStderrContaining(ERROR_TRACE);
        expect(disabled).toHaveFailed();
        expect(disabled.err).not.toContain(ERROR_TRACE);
    });

    it("honors OMNI_STDERR_LOG_ENABLED", async () => {
        const ws = failingWorkspace();

        const result = await runOmni(["--stderr-show-traces", "run", "nope"], {
            cwd: ws.cwd,
            env: { OMNI_STDERR_LOG_ENABLED: "true" },
        });

        expect(result).toHaveStderrContaining(ERROR_TRACE);
    });

    it("--stderr-show-traces toggles error traces on stderr", async () => {
        const ws = failingWorkspace();

        const withTraces = await runOmni(
            ["--stderr-log", "--stderr-show-traces", "run", "nope"],
            { cwd: ws.cwd },
        );
        const withoutTraces = await runOmni(["--stderr-log", "run", "nope"], {
            cwd: ws.cwd,
        });

        expect(withTraces).toHaveStderrContaining(ERROR_TRACE);
        expect(withoutTraces).toHaveFailed();
        expect(withoutTraces.err).not.toContain(ERROR_TRACE);
    });
});

describe("+global @cli (file traces)", () => {
    it("--file-trace-output + -f write JSON traces to the given file", async () => {
        const ws = envWorkspace();

        // Query a missing var so a warn-level event is actually written to the
        // file (an existing var logs nothing at the `info` file level).
        const result = await runOmni(
            [
                "-f",
                "info",
                "--file-trace-output",
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

        const result = await runOmni(["-f", "info", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(ws.exists(".omni/trace/omni.log")).toBe(true);
    });

    it("resolves a relative trace path against the workspace root", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { "sub/deep/.keep": "" },
        });

        const result = await runOmni(["-f", "info", "env", "get", "FOO"], {
            cwd: ws.path("sub", "deep"),
        });

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
            { cwd: ws.cwd },
        );
        expect(flag).toOutputContaining("stg");

        const fromEnv = await runOmni(
            ["-e", ".env.{ENV}", "env", "get", "PICK"],
            { cwd: ws.cwd, env: { OMNI_ENV: "staging" } },
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

describe("+global @cli (trace gating by stdout level)", () => {
    // The stdout fmt layer (and thus any native trace it would print) is only
    // installed when the stdout level is not `off` - see TracingSubscriber::new
    // in `crates/omni_tracing_subscriber/src/lib.rs`. So `-t` can only surface
    // traces when there is a stdout layer for them to attach to.
    const NATIVE_TRACE = "tracing_initialized";

    it("`-l off` suppresses native traces even with `-t`", async () => {
        const ws = envWorkspace();

        const off = await runOmni(["-l", "off", "-t", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });
        const on = await runOmni(["-l", "trace", "-t", "env", "get", "FOO"], {
            cwd: ws.cwd,
        });

        // No stdout layer at `off`, so the native trace never reaches stdout...
        expect(off.out).not.toContain(NATIVE_TRACE);
        // ...but it does once a stdout layer exists (here at `trace`).
        expect(on).toOutputContaining(NATIVE_TRACE);
    });
});

describe("+global @cli (stderr error logs + traces together)", () => {
    // The error *trace* the stderr log layer adds is prefixed `ERROR omni:`; the
    // eyre report (`Error:`) is the always-printed human-readable failure.
    const ERROR_TRACE = "ERROR omni:";

    function failingWorkspace() {
        return makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo hi" } },
            },
        });
    }

    it("`--stderr-log --stderr-show-traces` routes error logs and traces to stderr", async () => {
        const ws = failingWorkspace();

        const result = await runOmni(
            ["--stderr-log", "--stderr-show-traces", "run", "nope"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        // The error trace from the stderr log layer...
        expect(result).toHaveStderrContaining(ERROR_TRACE);
        // ...and the eyre report both land on stderr, together.
        expect(result).toHaveStderrContaining("Error:");
        // Neither leaks onto stdout.
        expect(result.out).not.toContain(ERROR_TRACE);
    });
});

describe("+global @cli (file traces at a requested level)", () => {
    it("`--file-trace-output <path>` + `-f debug` writes debug traces there", async () => {
        const ws = envWorkspace();

        const result = await runOmni(
            [
                "-f",
                "debug",
                "--file-trace-output",
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
    const MISSING_WARN = "environmental variable does not exists";

    it("`-l <level>` overrides OMNI_STDOUT_LOG_LEVEL", async () => {
        const ws = envWorkspace();

        // The env var silences stdout logs, but the explicit flag re-enables
        // warn-level output (so the missing-var warning appears).
        const overridden = await runOmni(
            ["-l", "warn", "env", "get", "MISSING"],
            { cwd: ws.cwd, env: { OMNI_STDOUT_LOG_LEVEL: "off" } },
        );
        const fromEnv = await runOmni(["env", "get", "MISSING"], {
            cwd: ws.cwd,
            env: { OMNI_STDOUT_LOG_LEVEL: "off" },
        });

        expect(overridden).toOutputContaining(MISSING_WARN);
        expect(fromEnv.out).not.toContain(MISSING_WARN);
    });

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
                "-l",
                "off",
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

    it("`-f <level>` overrides OMNI_FILE_TRACE_LEVEL", async () => {
        const ws = envWorkspace();

        // The env var disables file traces; the explicit flag turns them back
        // on, so the trace file is created.
        const result = await runOmni(
            [
                "-l",
                "off",
                "-f",
                "info",
                "--file-trace-output",
                "./override.log",
                "env",
                "get",
                "MISSING",
            ],
            { cwd: ws.cwd, env: { OMNI_FILE_TRACE_LEVEL: "off" } },
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
                "-l",
                "off",
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
