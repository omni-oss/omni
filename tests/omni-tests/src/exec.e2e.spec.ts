/**
 * `omni exec` e2e tests: running an ad-hoc command across matched projects.
 *
 * `exec` builds a temporary command task per matched project (dependencies are
 * ignored and the cache is bypassed by default), so a 2-project workspace runs
 * the command twice. The command after `--` is captured verbatim
 * (`trailing_var_arg` + `allow_hyphen_values`), so hyphen-leading args pass
 * through. Pinned to `crates/omni_cli_core/src/commands/exec.rs`.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

/** Two named projects (`app`, `other`) so project/dir filters are observable. */
function twoProjectSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "apps/app": { name: "app", tasks: { noop: "echo noop" } },
            other: { name: "other", tasks: { noop: "echo noop" } },
        },
    };
}

describe("+exec @e2e (run command across projects)", () => {
    it("`exec -- <cmd>` runs the command in every matched project", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(["exec", "--", "echo", "EXEC-MARK"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("EXEC-MARK");
        // One temp command task per matched project (app + other).
        expect(result).toOutputContaining("Successfully executed 2 tasks");
    });
});

describe("+exec @exitcode (exit code propagation)", () => {
    it("exit code reflects a successful command", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(
            ["exec", "-p", "app", "--", "echo", "ok"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
    });

    it("exit code reflects a failing command", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(["exec", "-p", "app", "--", "false"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
    });
});

describe("+exec @cli (argument passthrough)", () => {
    it("passes hyphen-leading args through verbatim", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(
            ["exec", "-p", "app", "--", "echo", "--weird", "-x"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("--weird -x");
    });
});

describe("+exec @e2e (project & dir filters)", () => {
    it("`-p/--project` limits exec to matching projects", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(
            ["exec", "-p", "app", "--", "echo", "hi"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Only `app` matched, so the command ran once.
        expect(result).toOutputContaining("Successfully executed 1 tasks");
    });

    it("`--dir` limits exec to projects under matching directories", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(
            ["exec", "--dir", "apps/**", "--", "echo", "hi"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Only `apps/app` lives under `apps/**`.
        expect(result).toOutputContaining("Successfully executed 1 tasks");
    });
});

describe("+exec @e2e (dry run)", () => {
    it("`--dry-run` prints the command without executing it", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(
            ["exec", "-p", "app", "--dry-run", "--", "echo", "DRY-MARK"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Dry run mode enabled");
        expect(result).toOutputContaining("Executing task");
        // The command is not actually run, so its output never appears.
        expect(result.stdout).not.toContain("DRY-MARK");
    });
});

describe("+exec @cli @exitcode (empty command)", () => {
    it("errors clearly when no command is given", async () => {
        const ws = makeWorkspace(twoProjectSpec());

        const result = await runOmni(["exec"], { cwd: ws.cwd });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no command provided to exec");
    });
});

/**
 * Three projects laid out so `-p`/`--dir` select overlapping-but-different
 * sets: `apps/app` (name `app`) is the only project that is BOTH under
 * `apps/**` AND whose name matches `app*`. `tools/app-tool` matches the name
 * glob but not the dir glob, and `apps/other` matches the dir glob but not the
 * name glob - so each filter alone keeps two projects while their
 * intersection keeps exactly one.
 */
function overlappingFiltersSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "apps/app": { name: "app", tasks: { noop: "echo noop" } },
            "tools/app-tool": {
                name: "app-tool",
                tasks: { noop: "echo noop" },
            },
            "apps/other": { name: "other", tasks: { noop: "echo noop" } },
        },
    };
}

describe("+exec @e2e (combined project & dir filters)", () => {
    it("`-p` and `--dir` together keep only projects matching BOTH", async () => {
        const ws = makeWorkspace(overlappingFiltersSpec());

        // `-p app*` alone -> {app, app-tool}; `--dir apps/**` alone ->
        // {app, other}. Filters compose as an intersection, so only `app`
        // survives and the command runs exactly once.
        const result = await runOmni(
            ["exec", "-p", "app*", "--dir", "apps/**", "--", "echo", "hi"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Successfully executed 1 tasks");
    });
});

describe("+exec @e2e (meta filter)", () => {
    it("`-m/--meta` filters matched projects by their meta configuration (CEL)", async () => {
        // `exec` builds a command task, so the meta filter is evaluated against
        // each project's meta block (not a task's). Both projects define `tier`
        // so the CEL expression resolves cleanly for every candidate.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    meta: { tier: "fast" },
                    tasks: { noop: "echo noop" },
                },
                other: {
                    name: "other",
                    meta: { tier: "slow" },
                    tasks: { noop: "echo noop" },
                },
            },
        });

        const result = await runOmni(
            ["exec", "-m", 'tier == "fast"', "--", "echo", "hi"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Only `app` carries `tier == "fast"`, so the command runs once.
        expect(result).toOutputContaining("Successfully executed 1 tasks");
    });
});

describe("+exec @e2e (arg injection)", () => {
    it("`-a KEY=VALUE` injects args into the ad-hoc command template", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { noop: "echo noop" } },
            },
        });

        // The trailing command becomes a temp task whose body is rendered with
        // the injected args, so `{{ args.subject }}` resolves before the shell
        // runs it.
        const result = await runOmni(
            [
                "exec",
                "-a",
                "subject=injected-arg",
                "--",
                "echo",
                "{{ args.subject }}",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("injected-arg");
    });
});

describe("+exec @perf (max concurrency)", () => {
    it("`-c/--max-concurrency` still runs the command in every matched project", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                a: { name: "a", tasks: { noop: "echo noop" } },
                b: { name: "b", tasks: { noop: "echo noop" } },
                c: { name: "c", tasks: { noop: "echo noop" } },
            },
        });

        // Bounding concurrency to 1 serializes the work but must not drop any
        // matched project.
        const result = await runOmni(["exec", "-c", "1", "--", "echo", "hi"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Successfully executed 3 tasks");
    });
});

describe("+exec @output (result file)", () => {
    it("`--result` writes execution results for the ad-hoc command task(s)", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { noop: "echo noop" } },
                other: { name: "other", tasks: { noop: "echo noop" } },
            },
        });

        const result = await runOmni(
            ["exec", "--result", "results.json", "--", "echo", "hi"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("results.json")).toBe(true);
        // One result entry per matched project's temp command task.
        const parsed = JSON.parse(ws.read("results.json"));
        expect(Array.isArray(parsed)).toBe(true);
        expect(parsed.length).toBe(2);
    });
});

describe("+exec @e2e (dry run with combined filters)", () => {
    it("`--dry-run` plans only the intersection of `-p` and `--dir` without executing", async () => {
        const ws = makeWorkspace(overlappingFiltersSpec());

        const result = await runOmni(
            [
                "exec",
                "--dry-run",
                "-p",
                "app*",
                "--dir",
                "apps/**",
                "--",
                "echo",
                "DRY-MARK",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("Dry run mode enabled");
        // Only `app` is in both filters; the planned temp task is namespaced
        // under its project.
        expect(result).toOutputContaining("app#exec");
        expect(result.stdout).not.toContain("other#exec");
        expect(result.stdout).not.toContain("app-tool#exec");
        // Nothing is actually executed in dry-run mode.
        expect(result.stdout).not.toContain("DRY-MARK");
    });
});
