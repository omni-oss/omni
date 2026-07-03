/**
 * `omni run` scm-affected filtering. The flags live in `RunArgs`
 * (`crates/omni_cli_core/src/commands/common_args.rs`) and the change detection
 * in `crates/omni_execution_plan/src/filter.rs`
 * (`DefaultTaskScmAffectedFilter`). A task is "affected" when one of its cache
 * key input files matches a file changed between base..target.
 *
 * Fresh temp workspaces are created outside any git repo, so enabling scm
 * filtering there surfaces a clear "no repository found" error - which is also
 * how the implicit-enable behavior of `--scm-base`/`--scm-target` is observed.
 * The change-selection test (scm-004) builds a real git repo and skips cleanly
 * when `git` is unavailable.
 */

import { execa } from "execa";
import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type Workspace } from "@/harness";

let gitProbe: Promise<boolean> | undefined;

/** Whether a usable `git` is on PATH (memoized once per file). */
function gitAvailable(): Promise<boolean> {
    gitProbe ??= execa("git", ["--version"], { reject: false })
        .then((r) => r.exitCode === 0)
        .catch(() => false);
    return gitProbe;
}

async function git(ws: Workspace, args: string[]): Promise<void> {
    await execa("git", args, { cwd: ws.cwd });
}

/** Initialize a git repo in the workspace with a deterministic identity. */
async function initRepo(ws: Workspace): Promise<void> {
    await git(ws, ["init", "-q"]);
    await git(ws, ["config", "user.email", "omni-tests@example.com"]);
    await git(ws, ["config", "user.name", "omni tests"]);
    await git(ws, ["config", "commit.gpgsign", "false"]);
}

describe("+scm @scm (enabling scm filtering)", () => {
    it("`--scm-affected` with no value defaults to auto detection", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        // Auto-detection runs against the (non-git) workspace and reports it.
        const result = await runOmni(["run", "build", "--scm-affected"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no repository found");
    });

    it("`-b/--scm-base` implicitly enables scm filtering", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        // No `--scm-affected`, yet providing a base turns on scm filtering -
        // proven by the scm error in this non-git workspace.
        const withBase = await runOmni(["run", "build", "-b", "HEAD"], {
            cwd: ws.cwd,
        });
        expect(withBase).toHaveFailed();
        expect(withBase).toHaveStderrContaining("no repository found");

        // Without the flag, scm filtering stays off and the task runs.
        const plain = await runOmni(["run", "build"], { cwd: ws.cwd });
        expect(plain).toHaveSucceeded();
    });

    it("`-t/--scm-target` implicitly enables scm filtering", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        const result = await runOmni(["run", "build", "-t", "HEAD"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no repository found");
    });
});

describe("+scm @scm @e2e (affected selection)", () => {
    it("selects only projects whose inputs changed between base..target", async (ctx) => {
        ctx.skip(!(await gitAvailable()), "git is not available");

        // Each project keys its cache on all of its files, so editing a file in
        // a project marks that project's `build` as affected.
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "BUILD-APP"',
                            cache: { key: { files: ["**/*"] } },
                        },
                    },
                },
                lib: {
                    name: "lib",
                    tasks: {
                        build: {
                            exec: 'echo "BUILD-LIB"',
                            cache: { key: { files: ["**/*"] } },
                        },
                    },
                },
            },
        });

        await initRepo(ws);
        await git(ws, ["add", "-A"]);
        await git(ws, ["commit", "-qm", "init"]);

        // Change only `app`, then commit.
        ws.write("app/changed.txt", "edited\n");
        await git(ws, ["add", "-A"]);
        await git(ws, ["commit", "-qm", "change-app"]);

        const result = await runOmni(
            [
                "run",
                "build",
                "--affected",
                "-b",
                "HEAD~1",
                "-t",
                "HEAD",
                "--output-logs=all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-APP");
        // `lib` was untouched, so its build is not selected.
        expect(result.stdout).not.toContain("BUILD-LIB");
    });
});

describe("+scm @scm @e2e (affected + filter combinations)", () => {
    /** Two projects that each key their cache on all of their files. */
    function affectedSpec() {
        return {
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        build: {
                            exec: 'echo "BUILD-APP"',
                            cache: { key: { files: ["**/*"] } },
                        },
                    },
                },
                lib: {
                    name: "lib",
                    tasks: {
                        build: {
                            exec: 'echo "BUILD-LIB"',
                            cache: { key: { files: ["**/*"] } },
                        },
                    },
                },
            },
        };
    }

    it("`--scm-affected` + `-p` intersect to affected projects matching the glob", async (ctx) => {
        ctx.skip(!(await gitAvailable()), "git is not available");

        const ws = makeWorkspace(affectedSpec());

        await initRepo(ws);
        await git(ws, ["add", "-A"]);
        await git(ws, ["commit", "-qm", "init"]);

        // Change BOTH projects so both are scm-affected.
        ws.write("app/changed.txt", "edited\n");
        ws.write("lib/changed.txt", "edited\n");
        await git(ws, ["add", "-A"]);
        await git(ws, ["commit", "-qm", "change-both"]);

        const result = await runOmni(
            [
                "run",
                "build",
                "--affected",
                "-b",
                "HEAD~1",
                "-t",
                "HEAD",
                "-p",
                "app",
                "--output-logs=all",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-APP");
        // `lib` is affected too, but the project glob narrows it out.
        expect(result.stdout).not.toContain("BUILD-LIB");
    });

    it("`-b` + `-t` together define the diff range and implicitly enable scm filtering", async (ctx) => {
        ctx.skip(!(await gitAvailable()), "git is not available");

        const ws = makeWorkspace(affectedSpec());

        await initRepo(ws);
        await git(ws, ["add", "-A"]);
        await git(ws, ["commit", "-qm", "init"]);

        // Change only `app`.
        ws.write("app/changed.txt", "edited\n");
        await git(ws, ["add", "-A"]);
        await git(ws, ["commit", "-qm", "change-app"]);

        // No `--affected`: providing both `-b` and `-t` implicitly enables it.
        const result = await runOmni(
            ["run", "build", "-b", "HEAD~1", "-t", "HEAD", "--output-logs=all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("BUILD-APP");
        expect(result.stdout).not.toContain("BUILD-LIB");
    });
});

describe("+scm @scm @exitcode (non-git workspace)", () => {
    it("fails gracefully with a clear error outside a repository", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: { name: "app", tasks: { build: "echo build" } },
            },
        });

        const result = await runOmni(["run", "build", "--affected"], {
            cwd: ws.cwd,
            timeout: 15_000,
        });

        // A clear error and a clean non-zero exit, not a panic or a hang.
        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("no repository found");
        expect(result.timedOut).toBe(false);
    });
});
