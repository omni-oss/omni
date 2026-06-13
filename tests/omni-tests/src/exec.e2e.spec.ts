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
