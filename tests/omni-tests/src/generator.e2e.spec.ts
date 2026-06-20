/**
 * `omni generator` (alias `gen`) - listing and non-interactive `run` behavior.
 * Pinned to `crates/omni_cli_core/src/commands/generator.rs` (+ its
 * `generator_common_args.rs`).
 *
 * The interactive paths (output-dir/project/save inputs, generator selection)
 * are covered by the PTY-driven `+prompt`/`+generator @tui` tests. Here we keep
 * runs non-interactive by always supplying `-n <name>`, an explicit output, and
 * either `--use-defaults` or `-v` so inputs never block. Because the fixture's
 * prompt is `remember: true`, the session is never empty, so a successful run
 * would otherwise pop the "save session?" confirm; we pass `--save-session` to
 * force the save and skip that prompt (the flag takes no value).
 */

import { describe, expect, it } from "vitest";
import {
    makeWorkspace,
    runOmni,
    scaffoldGeneratorSpec,
    skipUnlessRemoteReachable,
    skipUnlessSshReachable,
    spawnOmniPty,
    type WorkspaceSpec,
    workspaceMinimalRepo,
} from "@/harness";

/** Parse a generator session file (`.omni/generator.json`) as JSON. */
function parseSession(raw: string): Record<string, SessionEntry> {
    return JSON.parse(raw) as Record<string, SessionEntry>;
}

interface SessionEntry {
    targets: Record<string, string>;
    inputs: Record<string, unknown>;
}

describe("+generator @cli (list)", () => {
    it("`generator list` shows each generator's name and description", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(["generator", "list"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("scaffold");
        expect(result).toOutputContaining("scaffolds a greeting file");
    });

    it("`generator ls` is an alias of `list`", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const list = await runOmni(["generator", "list"], { cwd: ws.cwd });
        const ls = await runOmni(["generator", "ls"], { cwd: ws.cwd });

        expect(ls).toHaveSucceeded();
        expect(ls.out).toBe(list.out);
    });

    it("only discovers generators declared by the workspace `generators` config", async () => {
        // Same generator files on disk, but no `generators` source configured.
        const spec = scaffoldGeneratorSpec();
        const ws = makeWorkspace({ ...spec, workspace: { projects: ["**"] } });

        const result = await runOmni(["generator", "list"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        expect(result.stdout).not.toContain("scaffold");
    });
});

describe("+generator @cli (run)", () => {
    it("scaffolds files non-interactively with -n/-o/--use-defaults", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // `dest` target resolves to `@output/src`.
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");
    });

    it("-d/--dry-run makes no filesystem changes", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--dry-run",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("out/src/greeting.txt")).toBe(false);
        expect(ws.exists("out/.omni/generator.json")).toBe(false);
    });

    it("-p/--project writes into the project's directory", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-p",
                "app",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("app/src/greeting.txt")).toBe("Hello world!");
    });

    it("-v/--value prefills inputs (skipping the prompt) and --use-defaults uses defaults", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        // -v prefills `subject`, so the prompt is satisfied without input.
        const prefilled = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "filled",
                "-v",
                "subject=Custom",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(prefilled).toHaveSucceeded();
        expect(ws.read("filled/src/greeting.txt")).toBe("Hello Custom!");

        const defaulted = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "defaulted",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(defaulted).toHaveSucceeded();
        expect(ws.read("defaulted/src/greeting.txt")).toBe("Hello world!");
    });

    it("-t/--target overrides the generator's target output path", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-t",
                "dest=lib",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Override redirects `dest` from `@output/src` to `out/lib`.
        expect(ws.read("out/lib/greeting.txt")).toBe("Hello world!");
        expect(ws.exists("out/src/greeting.txt")).toBe(false);
    });

    it("--overwrite never/always controls existing-file behavior", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        ws.write("out/src/greeting.txt", "OLD");

        const never = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--overwrite",
                "never",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(never).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("OLD");

        const always = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--overwrite",
                "always",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(always).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");
    });

    it("--save-session writes .omni/generator.json with inputs and targets", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-t",
                "dest=lib",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        const session = parseSession(ws.read("out/.omni/generator.json"));
        // Only `remember: true` inputs and overridden targets are persisted.
        expect(session?.scaffold?.inputs.subject).toBe("world");
        expect(session?.scaffold?.targets.dest).toBe("lib");
    });

    it("re-runs restore the saved session; --ignore-session bypasses it", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        // Seed a session by running once with a non-default subject.
        const seed = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-v",
                "subject=Alice",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(seed).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello Alice!");

        // Re-run with no value and no --use-defaults: the session restores
        // `subject=Alice`, so the prompt is skipped instead of blocking.
        ws.write("out/src/greeting.txt", "STALE");
        const restored = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--overwrite",
                "always",
            ],
            { cwd: ws.cwd },
        );
        expect(restored).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello Alice!");

        // `--ignore-session=true` skips the saved value and falls back to the
        // default.
        const ignored = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--ignore-session=true",
                "--use-defaults",
                "--overwrite",
                "always",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(ignored).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");
    });

    it("prevents non-user invocable generators from being run", async () => {
        const spec = scaffoldGeneratorSpec();
        if (spec.projects) {
            spec.projects["generators/non-user-invocable/generator.omni.yaml"] =
                {
                    name: "non-user-invocable",
                    description: "a generator that can't be run by the user",
                    user_invocable: false,
                    actions: [],
                };
        }
        const ws = makeWorkspace(spec);

        const result = await runOmni(
            ["generator", "run", "-n", "non-user-invocable", "-o", "out"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(1);
        expect(result.stderr).toContain(
            "generator 'non-user-invocable' is not invocable by the user",
        );
    });
});

describe("+generator @cli (--save-session/--ignore-session value handling)", () => {
    // Both flags are optional-value booleans: bare = true, `=true`/`=false`
    // explicit, and the space-separated form is rejected (require_equals).
    it("bare --save-session defaults to true and writes the session", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("out/.omni/generator.json")).toBe(true);
    });

    it("--save-session=false skips writing the session", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");
        expect(ws.exists("out/.omni/generator.json")).toBe(false);
    });

    it("--save-session=true writes the session", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=true",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.exists("out/.omni/generator.json")).toBe(true);
    });

    it("--save-session with a space-separated value is rejected (require_equals)", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--save-session",
                "false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(2);
    });

    it("--ignore-session=true bypasses the saved session; =false restores it", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        // Seed a session whose subject differs from the default.
        const seed = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-v",
                "subject=Seeded",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(seed).toHaveSucceeded();

        // =true ignores the session and uses the prompt default instead.
        const ignored = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--ignore-session=true",
                "--use-defaults",
                "--overwrite",
                "always",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );
        expect(ignored).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");

        // =false (the default) honors the saved session value.
        const restored = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--ignore-session=false",
                "--overwrite",
                "always",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );
        expect(restored).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello Seeded!");
    });

    it("bare --ignore-session defaults to true and bypasses the saved session", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const seed = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-v",
                "subject=Seeded",
                "--use-defaults",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );
        expect(seed).toHaveSucceeded();

        const ignored = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--ignore-session",
                "--use-defaults",
                "--overwrite",
                "always",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );
        expect(ignored).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");
    });

    it("--ignore-session with a space-separated value is rejected (require_equals)", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--ignore-session",
                "true",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(2);
    });
});

describe("+generator @exitcode (run errors)", () => {
    it("-p/--project with an unknown project errors with `Project <x> not found`", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-p",
                "nope",
                "--use-defaults",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(1);
        expect(result).toHaveStderrContaining("Project nope not found");
    });

    it("-o and -p together is rejected as a clap conflict", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        // The `project` arg declares `conflicts_with = "output"`, so clap
        // rejects the combination before the runtime "use --output" warning
        // branch in generator.rs can ever run.
        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-p",
                "app",
                "--use-defaults",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(2);
        expect(result).toHaveStderrContaining(
            "cannot be used with '--project <PROJECT>'",
        );
    });
});

/**
 * A workspace whose `pipeline` generator writes lowercase files and then pipes
 * them back through `tr a-z A-Z`, exercising the `transform`/`transform-many`
 * actions. It has no inputs, so the session stays empty and no save confirm
 * appears. `tr` is POSIX-only, hence the platform guard.
 */
function transformSpec() {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/pipeline/generator.omni.yaml": {
                name: "pipeline",
                actions: [
                    {
                        type: "add-content",
                        output_path: "greeting.txt",
                        content: "hello world",
                    },
                    {
                        type: "add-content",
                        output_path: "nested/again.txt",
                        content: "abc def",
                    },
                    {
                        type: "transform",
                        file: "greeting.txt",
                        command: "tr a-z A-Z",
                    },
                    {
                        type: "transform-many",
                        files: ["**/*.txt", "!greeting.txt"],
                        command: "tr a-z A-Z",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

describe("+generator @e2e (transform actions)", () => {
    it.skipIf(process.platform === "win32")(
        "transform/transform-many pipe generated files through a command",
        async () => {
            const ws = makeWorkspace(transformSpec());

            const result = await runOmni(
                ["generator", "run", "-n", "pipeline", "-o", "out"],
                { cwd: ws.cwd },
            );

            expect(result).toHaveSucceeded();
            // `transform` uppercases the single named file...
            expect(ws.read("out/greeting.txt")).toBe("HELLO WORLD");
            // ...and `transform-many` uppercases every glob match except the
            // excluded `greeting.txt`.
            expect(ws.read("out/nested/again.txt")).toBe("ABC DEF");
        },
    );
});

describe("+generator @tui (interactive run via PTY)", () => {
    it("prompts for output target, generator name, inputs, then save", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        // No -o/-p/-n: omni must drive the whole interactive chain.
        const pty = spawnOmniPty(["generator", "run"], { cwd: ws.cwd });

        // 1. Output-target select: move to "Project directory" and confirm.
        await pty.waitFor("Where should the generator output be written?");
        pty.press("down");
        pty.press("enter");

        // 2. Project select (only `app`).
        await pty.waitFor("Select project");
        pty.press("enter");

        // 3. Generator-name select (only `scaffold`).
        await pty.waitFor("Select generator");
        pty.press("enter");

        // 4. The generator's own `subject` prompt.
        await pty.waitFor("Who to greet?");
        pty.type("PTY");
        pty.press("enter");

        // 5. Post-run "save session?" confirm (defaults to yes).
        await pty.waitFor("save inputs and targets");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        // Output landed in the chosen project's `dest` target dir, and the save
        // confirm persisted the session.
        expect(ws.read("app/src/greeting.txt")).toBe("Hello PTY!");
        expect(ws.exists("app/.omni/generator.json")).toBe(true);
    });
});

/**
 * A workspace whose `generators` config points at a real git remote. Pulling
 * is what gen-014 exercises: the repo is cloned into
 * `.omni/sources/generator/git/<slug>/<rev>/`, locked in
 * `.omni/sources/generator/lock.json`, and its generators become discoverable.
 */
function gitGeneratorSourceSpec() {
    return {
        workspace: {
            projects: ["**"],
            generators: [
                {
                    source: "git",
                    uri: workspaceMinimalRepo.https,
                    rev: workspaceMinimalRepo.rev,
                },
            ],
        },
    };
}

/** Same as {@link gitGeneratorSourceSpec} but over the `ssh://` URL form. */
function sshGitGeneratorSourceSpec() {
    return {
        workspace: {
            projects: ["**"],
            generators: [
                {
                    source: "git",
                    uri: workspaceMinimalRepo.sshUrl,
                    rev: workspaceMinimalRepo.rev,
                },
            ],
        },
    };
}

function parseLockfile(raw: string): {
    git: Record<string, Record<string, { commit: string }>>;
} {
    return JSON.parse(raw);
}

describe("+generator @e2e (git sources)", () => {
    const CLONE_TIMEOUT_MS = 10_000;

    it(
        "pulls a git source, locks it, and exposes its generators to `list`",
        async (ctx) => {
            await skipUnlessRemoteReachable(ctx);

            const ws = makeWorkspace(gitGeneratorSourceSpec());

            const result = await runOmni(["generator", "list"], {
                cwd: ws.cwd,
                timeout: CLONE_TIMEOUT_MS,
            });

            expect(result).toHaveSucceeded();
            // The git-sourced generator is discovered and listed.
            expect(result).toOutputContaining(
                workspaceMinimalRepo.generatorDisplayName,
            );
            expect(result).toOutputContaining(workspaceMinimalRepo.generatorId);

            // The pull is recorded in the lockfile with a resolved commit.
            const lockPath = ".omni/sources/generator/lock.json";
            expect(ws.exists(lockPath)).toBe(true);
            const lock = parseLockfile(ws.read(lockPath));
            const revs = lock.git[workspaceMinimalRepo.https];
            expect(revs).toBeDefined();
            expect(revs?.[workspaceMinimalRepo.rev]?.commit).toMatch(
                /^[0-9a-f]{40}$/,
            );
        },
        CLONE_TIMEOUT_MS,
    );

    it(
        "runs a generator resolved from a git source",
        async (ctx) => {
            await skipUnlessRemoteReachable(ctx);

            const ws = makeWorkspace(gitGeneratorSourceSpec());

            const result = await runOmni(
                [
                    "generator",
                    "run",
                    "-n",
                    workspaceMinimalRepo.generatorId,
                    "-o",
                    "out",
                    "-v",
                    `${workspaceMinimalRepo.promptName}=from-git`,
                    "--use-defaults",
                    "--save-session=false",
                ],
                { cwd: ws.cwd, timeout: CLONE_TIMEOUT_MS },
            );

            expect(result).toHaveSucceeded();
            // The git generator's `add` action renders the workspace template
            // with our prefilled prompt value.
            expect(ws.read("out/workspace.omni.yaml")).toContain(
                "name: from-git",
            );
        },
        CLONE_TIMEOUT_MS,
    );

    it(
        "pulls and locks an `ssh://` git source using the machine's keys",
        async (ctx) => {
            // The SSH transport goes through the system `ssh` (gix shells out
            // to it). Only the `ssh://` URL form is a valid `uri`; the SCP form
            // isn't a URL. Gated on SSH access so it skips on CI.
            await skipUnlessSshReachable(ctx);

            const ws = makeWorkspace(sshGitGeneratorSourceSpec());

            const result = await runOmni(["generator", "list"], {
                cwd: ws.cwd,
                timeout: CLONE_TIMEOUT_MS,
            });

            expect(result).toHaveSucceeded();
            expect(result).toOutputContaining(workspaceMinimalRepo.generatorId);

            const lockPath = ".omni/sources/generator/lock.json";
            const lock = parseLockfile(ws.read(lockPath));
            // The lockfile keys the source by its `ssh://` URI.
            const revs = lock.git[workspaceMinimalRepo.sshUrl];
            expect(revs).toBeDefined();
            expect(revs?.[workspaceMinimalRepo.rev]?.commit).toMatch(
                /^[0-9a-f]{40}$/,
            );
        },
        CLONE_TIMEOUT_MS,
    );
});

describe("+generator @exitcode (validation)", () => {
    it("errors when two generators share the same name", async () => {
        // `validate` runs at the top of `run_named`, before any prompting, so a
        // duplicate name fails fast regardless of the generators' inputs.
        const dupGenerator = (greeting: string) => ({
            name: "dup",
            description: "duplicate-named generator",
            actions: [
                {
                    type: "add-content",
                    output_path: "greeting.txt",
                    content: greeting,
                },
            ],
        });
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: [{ source: "local", path: "generators/**" }],
            },
            projects: {
                "generators/a/generator.omni.yaml": dupGenerator("from a"),
                "generators/b/generator.omni.yaml": dupGenerator("from b"),
            },
            files: { ".omni/sources/generator/.keep": "" },
        });

        const result = await runOmni(
            ["generator", "run", "-n", "dup", "-o", "out", "--use-defaults"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("generator names must be unique");
    });
});

/**
 * A `multi` generator with two `remember: true` inputs (`subject`,
 * `salutation`) and two targets (`dest` -> @output/src, `other` -> @output/lib),
 * writing one file into each target. Two slots of each kind let us prove that
 * repeated `-v K=V` and `-t name=path` flags ALL apply (not just the last one).
 */
function multiSlotGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            app: { name: "app", tasks: { build: 'echo "build app"' } },
            "generators/multi/generator.omni.yaml": {
                name: "multi",
                description: "scaffolds two greeting files",
                inputs: [
                    {
                        type: "text",
                        name: "subject",
                        message: "Who to greet?",
                        default: "world",
                        remember: true,
                    },
                    {
                        type: "text",
                        name: "salutation",
                        message: "How to greet?",
                        default: "Hello",
                        remember: true,
                    },
                ],
                targets: { dest: "@output/src", other: "@output/lib" },
                actions: [
                    {
                        type: "add-content",
                        output_path: "greeting.txt",
                        target: "dest",
                        content:
                            "{{ inputs.salutation }} {{ inputs.subject }}!",
                    },
                    {
                        type: "add-content",
                        output_path: "other.txt",
                        target: "other",
                        content: "{{ inputs.subject }}",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

describe("+generator @cli (run flag combinations)", () => {
    it("-v + -t + --use-defaults combine prefill and target override non-interactively", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-v",
                "subject=Custom",
                "-t",
                "dest=lib",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // -v wins over the prompt default even with --use-defaults, and -t
        // redirects `dest` from @output/src to out/lib.
        expect(ws.read("out/lib/greeting.txt")).toBe("Hello Custom!");
        expect(ws.exists("out/src/greeting.txt")).toBe(false);
    });

    it("multiple -v and multiple -t flags all apply", async () => {
        const ws = makeWorkspace(multiSlotGeneratorSpec());

        // Both prompts are prefilled, so the run is non-interactive without
        // --use-defaults.
        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "multi",
                "-o",
                "out",
                "-v",
                "subject=Alice",
                "-v",
                "salutation=Hi",
                "-t",
                "dest=a",
                "-t",
                "other=b",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Both prefilled values and both target overrides take effect.
        expect(ws.read("out/a/greeting.txt")).toBe("Hi Alice!");
        expect(ws.read("out/b/other.txt")).toBe("Alice");
    });

    it("-p with -v and --overwrite always overwrites files in the project directory", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        // Pre-create the file the `dest` target resolves to inside the project.
        ws.write("app/src/greeting.txt", "OLD");

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-p",
                "app",
                "-v",
                "subject=Bob",
                "--overwrite",
                "always",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // `always` replaces the pre-existing file with the generated content.
        expect(ws.read("app/src/greeting.txt")).toBe("Hello Bob!");
    });

    it("--use-defaults --save-session=false runs with defaults and writes no session", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/src/greeting.txt")).toBe("Hello world!");
        // The session is skipped even though `subject` is `remember: true`.
        expect(ws.exists("out/.omni/generator.json")).toBe(false);
    });

    it("-o --overwrite never leaves pre-existing target files untouched", async () => {
        const ws = makeWorkspace(scaffoldGeneratorSpec());
        ws.write("out/src/greeting.txt", "OLD");

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "--overwrite",
                "never",
                "-v",
                "subject=Bob",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // `never` keeps the existing file even though -v would have changed it.
        expect(ws.read("out/src/greeting.txt")).toBe("OLD");
    });

    it("-v KEY= (empty) and -v KEY=a=b mirror parse_key_value edge cases", async () => {
        // Empty value: parse_key_value allows an empty value, so the prompt is
        // prefilled with "" (and therefore skipped, not asked).
        const empty = makeWorkspace(scaffoldGeneratorSpec());
        const emptyResult = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-v",
                "subject=",
                "--save-session=false",
            ],
            { cwd: empty.cwd },
        );
        expect(emptyResult).toHaveSucceeded();
        expect(empty.read("out/src/greeting.txt")).toBe("Hello !");

        // Value containing `=`: parse_key_value splits on the FIRST `=`, so the
        // remainder (`a=b`) is kept verbatim as the value.
        const eq = makeWorkspace(scaffoldGeneratorSpec());
        const eqResult = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "scaffold",
                "-o",
                "out",
                "-v",
                "subject=a=b",
                "--save-session=false",
            ],
            { cwd: eq.cwd },
        );
        expect(eqResult).toHaveSucceeded();
        expect(eq.read("out/src/greeting.txt")).toBe("Hello a=b!");
    });
});

// ---------------------------------------------------------------------------
// Recursion detection + --max-depth (configurable nesting limit)
//
// Mirrors the MCP-side coverage in `mcp.e2e.spec.ts`, exercised here through
// the `omni generator run` CLI. `detect_recursion` runs up front in
// `run_in_transaction`; the runtime depth cap is the configurable backstop
// surfaced via `--max-depth`.
// ---------------------------------------------------------------------------

/** A generator that invokes itself, forming a direct recursion cycle. */
function selfRecursiveGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/loop/generator.omni.yaml": {
                name: "loop",
                description: "calls itself, forming a direct recursion cycle",
                inputs: [],
                actions: [{ type: "run-generator", generator: "loop" }],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/** Two generators that invoke each other (ping → pong → ping). */
function mutualRecursionGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/ping/generator.omni.yaml": {
                name: "ping",
                inputs: [],
                actions: [{ type: "run-generator", generator: "pong" }],
            },
            "generators/pong/generator.omni.yaml": {
                name: "pong",
                inputs: [],
                actions: [{ type: "run-generator", generator: "ping" }],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/** A legitimate, non-cyclic chain: parent runs child, which writes a file. */
function nestedGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/parent/generator.omni.yaml": {
                name: "parent",
                inputs: [],
                actions: [{ type: "run-generator", generator: "child" }],
            },
            "generators/child/generator.omni.yaml": {
                name: "child",
                inputs: [],
                actions: [
                    {
                        type: "add-content",
                        output_path: "nested.txt",
                        content: "from child",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

describe("+generator @exitcode (recursion)", () => {
    it("rejects a generator that directly invokes itself", async () => {
        // loop → loop. detect_recursion fails before any action runs, so no
        // output is produced.
        const ws = makeWorkspace(selfRecursiveGeneratorSpec());

        const result = await runOmni(
            ["generator", "run", "-n", "loop", "-o", "out", "--use-defaults"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(1);
        expect(result.stderr).toContain("will recurse into itself");
        expect(ws.exists("out")).toBe(false);
    });

    it("rejects a generator caught in an indirect (mutual) recursion cycle", async () => {
        // ping → pong → ping.
        const ws = makeWorkspace(mutualRecursionGeneratorSpec());

        const result = await runOmni(
            ["generator", "run", "-n", "ping", "-o", "out", "--use-defaults"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(1);
        expect(result.stderr).toContain("will recurse into itself");
    });
});

describe("+generator @cli (--max-depth)", () => {
    it("aborts a legitimate (acyclic) chain when --max-depth is below its nesting", async () => {
        // parent → child is non-cyclic; child runs at depth 1. With
        // --max-depth 0 the nested run exceeds the cap and is rejected even
        // though there is no recursion.
        const ws = makeWorkspace(nestedGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--max-depth",
                "0",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveExitCode(1);
        expect(result.stderr).toContain("exceeded the maximum nesting depth");
        expect(ws.exists("out/nested.txt")).toBe(false);
    });

    it("runs the chain when --max-depth is raised to allow the nesting", async () => {
        const ws = makeWorkspace(nestedGeneratorSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--max-depth",
                "5",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/nested.txt")).toContain("from child");
    });

    it("runs the chain with the default depth when --max-depth is omitted", async () => {
        const ws = makeWorkspace(nestedGeneratorSpec());

        const result = await runOmni(
            ["generator", "run", "-n", "parent", "-o", "out", "--use-defaults"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/nested.txt")).toContain("from child");
    });
});
