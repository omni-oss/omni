/**
 * Structured task command e2e tests (RFC 0001: `string | argv`).
 *
 * These pin the behaviors the structured-command work guarantees:
 *
 *   1. A shell-string `exec` is split with `shlex` uniformly on every platform,
 *      so a single-quoted section stays ONE argument (the historical Windows
 *      bug split it on interior spaces).
 *   2. An argv `exec` (a YAML sequence) is passed through verbatim - an element
 *      containing spaces is never re-split.
 *   3. A command that resolves to an EMPTY argv (empty shell string, or empty
 *      sequence) executes as a no-op and is counted as SUCCEEDED, whereas a
 *      task with NO command at all (`exec` absent/null) is SKIPPED with a
 *      `no command to execute` reason. Both exit 0, but the empty case must
 *      still go through execution (it once panicked indexing `argv[0]`), and it
 *      must stay distinct from the skipped null case.
 *
 * Argument boundaries are made observable with a tiny `args.js` helper that
 * prints one line per received argv element, so "one spaced arg" vs. "several
 * split args" is directly assertable rather than relying on `echo` (which
 * collapses argv back into a single space-joined line).
 *
 * Pinned to `crates/omni_command_config` (parse/resolve) and the execution
 * path in `crates/omni_process`.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

/**
 * Prints each received argument on its own line, wrapped in `<...>`, plus a
 * count. `node args.js "a b" c` therefore emits `ARGC=2`, `ARG0=<a b>`,
 * `ARG1=<c>` - which distinguishes a preserved spaced argument from one that
 * was split on its interior space.
 */
const ARGS_JS = [
    "const args = process.argv.slice(2);",
    'console.log("ARGC=" + args.length);',
    'args.forEach((a, i) => console.log("ARG" + i + "=<" + a + ">"));',
    "",
].join("\n");

/** Shell-string form: the quoted section must survive as a single argument. */
function shellSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: {
                    // Short form -> CommandConfig::Shell -> shlex split.
                    cmd: "node args.js 'hello world' extra",
                },
            },
        },
        files: { "app/args.js": ARGS_JS },
    };
}

/** Argv form: the spaced element is passed through verbatim (never split). */
function argvSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: {
                    // Long form with a sequence exec -> CommandConfig::Argv.
                    cmd: { exec: ["node", "args.js", "hello world", "extra"] },
                },
            },
        },
        files: { "app/args.js": ARGS_JS },
    };
}

/**
 * Empty shell string: resolves to an empty argv (no program). Must be treated
 * as a successful no-op rather than panicking on a missing `argv[0]`.
 */
function emptyShellSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: { cmd: "" },
            },
        },
    };
}

/** Empty argv sequence: also resolves to no program; same no-op success. */
function emptyArgvSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: { cmd: { exec: [] } },
            },
        },
    };
}

/**
 * Long-form task with no `exec` at all: a null command, distinct from the empty
 * cases above - there is nothing to run, so the task is skipped.
 */
function nullCommandSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: { cmd: { description: "no command configured" } },
            },
        },
    };
}

describe("+command @e2e (shell string keeps single-quoted arg intact)", () => {
    it("splits a single-quoted section into exactly one argument on every platform", async () => {
        const ws = makeWorkspace(shellSpec());

        const result = await runOmni(["run", "cmd", "--output-logs=all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("ARGC=2");
        expect(result).toOutputContaining("ARG0=<hello world>");
        expect(result).toOutputContaining("ARG1=<extra>");
    });
});

describe("+command @e2e (argv form passes spaced element verbatim)", () => {
    it("never re-splits an argv element that contains spaces", async () => {
        const ws = makeWorkspace(argvSpec());

        const result = await runOmni(["run", "cmd", "--output-logs=all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("ARGC=2");
        expect(result).toOutputContaining("ARG0=<hello world>");
        expect(result).toOutputContaining("ARG1=<extra>");
    });
});

describe("+command @e2e (empty command executes as a successful no-op)", () => {
    it("an empty shell string executes (counted as succeeded) instead of panicking", async () => {
        const ws = makeWorkspace(emptyShellSpec());

        const result = await runOmni(["run", "cmd"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        // It actually ran (empty_command_is_success), it was NOT skipped.
        expect(result.stdout).not.toContain("no command to execute");
        expect(result).toMatchOutput(/Succeeded[^\n]*1/);
    });

    it("an empty argv sequence executes (counted as succeeded) instead of panicking", async () => {
        const ws = makeWorkspace(emptyArgvSpec());

        const result = await runOmni(["run", "cmd"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("no command to execute");
        expect(result).toMatchOutput(/Succeeded[^\n]*1/);
    });
});

describe("+command @e2e (null command is skipped, not executed)", () => {
    it("a task with no exec is skipped with a 'no command' reason and still exits 0", async () => {
        const ws = makeWorkspace(nullCommandSpec());

        const result = await runOmni(["run", "cmd"], { cwd: ws.cwd });

        // A missing command is a skip, not a failure and not a no-op success.
        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("no command to execute");
        expect(result).toMatchOutput(/Skipped[^\n]*1/);
    });
});
