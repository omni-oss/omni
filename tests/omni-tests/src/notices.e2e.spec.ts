/**
 * Events, diagnostics, notices and output: what the CLI prints,
 * where, and what `-q` and `omni code` do.
 *
 * Pinned to `crates/omni_cli_core/src/subscriber.rs` (the CLI presenter) and
 * `crates/omni_cli_core/src/commands/code.rs`.
 */

import { describe, expect, it } from "vitest";

import {
    connectMcp,
    makeWorkspace,
    runOmni,
    singleProjectSpec,
    type WorkspaceSpec,
} from "@/harness";

/** `web` (kind: unit), `api` (kind: e2e) and `docs` (no meta at all). */
function metaSpec(): WorkspaceSpec {
    const task = (name: string, kind?: string) => ({
        exec: `echo "RAN-${name}"`,
        ...(kind ? { meta: { kind } } : {}),
    });
    return {
        workspace: { projects: ["**"] },
        projects: {
            web: { name: "web", tasks: { test: task("web", "unit") } },
            api: { name: "api", tasks: { test: task("api", "e2e") } },
            docs: { name: "docs", tasks: { test: task("docs") } },
        },
    };
}

function failingSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: { ok: 'echo "OK-RAN"', bad: "exit 3" },
            },
        },
    };
}

describe("+notices @cli (stdout is results, stderr is narration)", () => {
    it("a run prints task output on stdout and omni's own lines on stderr", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["run", "build", "--output-logs=all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("build app");
        expect(result.stdout).not.toContain("Executed task");
        expect(result).toHaveStderrContaining("Executed task 'app#build'");
        expect(result).toHaveStderrMatching(/Succeeded[^\n]*1/);
    });

    it("`cache dir` and `project list` keep stdout to results, with and without --trace", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        for (const extra of [[], ["--trace", "debug"]]) {
            const list = await runOmni([...extra, "project", "list"], {
                cwd: ws.cwd,
            });
            expect(list).toHaveSucceeded();
            expect(list.stdout).toContain("app");
            expect(list.stdout).not.toMatch(/\b(INFO|DEBUG|TRACE|WARN)\b/);

            const dir = await runOmni([...extra, "cache", "dir"], {
                cwd: ws.cwd,
            });
            expect(dir).toHaveSucceeded();
            expect(dir.stdout).not.toMatch(/\b(INFO|DEBUG|TRACE|WARN)\b/);
        }
    });
});

describe("+notices @cli (dry run)", () => {
    it("has no header, and the command line is a result that `-q` keeps", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const loud = await runOmni(["run", "build", "-d", "--force"], {
            cwd: ws.cwd,
        });
        const quiet = await runOmni(["-q", "run", "build", "-d", "--force"], {
            cwd: ws.cwd,
        });

        for (const result of [loud, quiet]) {
            expect(result).toHaveSucceeded();
            expect(result).toOutputContaining("Executing task 'app#build':");
            expect(result.out).not.toContain("Dry run mode enabled");
            expect(result.err).not.toContain("Dry run mode enabled");
        }
    });
});

describe("+notices @cli (-q)", () => {
    it("applies quiet output options when the flag follows the subcommand", async () => {
        const ws = makeWorkspace(failingSpec());
        const result = await runOmni(
            ["run", "ok", "--quiet", "--output-logs=all"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OK-RAN");
        expect(result.err).not.toContain("Executed task");
        expect(result.err).not.toMatch(/Succeeded/);
    });

    it("hides lifecycle lines and the summary on success, keeping task output", async () => {
        const ws = makeWorkspace(failingSpec());

        const result = await runOmni(["-q", "run", "ok", "--output-logs=all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("OK-RAN");
        expect(result.err).not.toContain("Executed task");
        expect(result.err).not.toMatch(/Succeeded/);
    });

    it("still shows failures, the summary table on failure, and the exit status", async () => {
        const ws = makeWorkspace(failingSpec());

        const result = await runOmni(["-q", "run", "bad"], { cwd: ws.cwd });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("Task 'app#bad' error");
        expect(result).toHaveStderrMatching(/Failed[^\n]*1/);
    });

    it("hides a warn notice but keeps an `error` notice and an `essential` one", async () => {
        const meta = makeWorkspace(metaSpec());

        // `meta.filter_excluded` is level `error`: shown even under -q.
        const excluded = await runOmni(
            [
                "-q",
                "run",
                "test",
                "--meta",
                'kind == "e2e"',
                "--meta-on-error",
                "exclude",
            ],
            { cwd: meta.cwd },
        );
        expect(excluded).toHaveSucceeded();
        expect(excluded).toHaveStderrContaining("error[meta.filter_excluded]");

        // `redact.disabled` is `essential`: shown even under -q.
        const ws = makeWorkspace(singleProjectSpec());
        const essential = await runOmni(
            ["-q", "run", "build", "--redact", "none"],
            { cwd: ws.cwd },
        );
        expect(essential).toHaveSucceeded();
        expect(essential).toHaveStderrContaining("warning[redact.disabled]");

        // `env.var_missing` is a plain warning: hidden.
        const missing = await runOmni(["-q", "env", "get", "NOPE"], {
            cwd: ws.cwd,
        });
        expect(missing.err).not.toContain("env.var_missing");
    });

    it("never changes the exit status", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const loud = await runOmni(["env", "get", "NOPE"], { cwd: ws.cwd });
        const quiet = await runOmni(["-q", "env", "get", "NOPE"], {
            cwd: ws.cwd,
        });

        expect(loud.exitCode).toBe(quiet.exitCode);
    });

    it("`--ui stream` prints task output only, `--ui progress` keeps capture and replay", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const stream = await runOmni(
            ["-q", "run", "build", "--ui", "stream", "--output-logs=all"],
            { cwd: ws.cwd },
        );
        expect(stream).toHaveSucceeded();
        expect(stream).toOutputContaining("build app");
        expect(stream.err.trim()).toBe("");

        const progress = await runOmni(
            ["-q", "run", "build", "--ui", "progress", "--output-logs=all"],
            { cwd: ws.cwd },
        );
        expect(progress).toHaveSucceeded();
        expect(progress).toOutputContaining("build app");
    });
});

describe("+notices @cli (redaction)", () => {
    it("`--redact none` is reported once, as an essential warning with its code", async () => {
        const ws = makeWorkspace(singleProjectSpec());

        const result = await runOmni(["run", "build", "--redact", "none"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining("warning[redact.disabled]");
        expect(result).toHaveStderrContaining(
            "omni code explain redact.disabled",
        );
        const count = (result.err.match(/warning\[redact\.disabled\]/g) ?? [])
            .length;
        expect(count).toBe(1);
    });
});

describe("+code @cli (explain)", () => {
    it("explains an error code and a notice code", async () => {
        const error = await runOmni(["code", "explain", "meta.filter_failed"]);
        expect(error).toHaveSucceeded();
        expect(error).toOutputContaining("meta.filter_failed");
        expect(error).toOutputContaining("error");
        // `related` points at the tolerated form of the same condition.
        expect(error).toOutputContaining("meta.filter_excluded");

        const notice = await runOmni([
            "code",
            "explain",
            "sandbox.os_disabled",
        ]);
        expect(notice).toHaveSucceeded();
        expect(notice).toOutputContaining("notice");
        expect(notice).toOutputContaining("essential:  true");
    });

    it("`--format json` matches the MCP `code_explain` tool", async () => {
        const cli = await runOmni([
            "code",
            "explain",
            "--format",
            "json",
            "meta.filter_excluded",
        ]);
        expect(cli).toHaveSucceeded();

        const ws = makeWorkspace(singleProjectSpec());
        const { client } = await connectMcp({ cwd: ws.cwd });
        const tool = await client.callTool({
            name: "code_explain",
            arguments: { code: "meta.filter_excluded" },
        });

        expect(JSON.parse(cli.stdout)).toEqual(tool.structuredContent);
    });

    it("an unknown code is a usage error that lists close matches", async () => {
        const result = await runOmni(["code", "explain", "meta.filter_faild"]);

        expect(result).toHaveFailed();
        expect(result.exitCode).toBe(2);
        expect(result).toHaveStderrContaining("unknown code");
        expect(result).toHaveStderrContaining("meta.filter_failed");
    });
});

describe("+code @cli (list)", () => {
    it("`code list --format json` lists error and notice codes, valid and unique", async () => {
        const result = await runOmni(["code", "list", "--format", "json"]);

        expect(result).toHaveSucceeded();
        const entries = JSON.parse(result.stdout) as Array<{
            code: string;
            kind: string;
        }>;
        const codes = entries.map((e) => e.code);
        expect(new Set(codes).size).toBe(codes.length);
        expect(codes).toContain("meta.filter_failed");
        expect(codes).toContain("sandbox.os_disabled");
        expect(new Set(entries.map((e) => e.kind))).toEqual(
            new Set(["error", "notice"]),
        );
        for (const code of codes) {
            expect(code).toMatch(/^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/);
        }
    });

    it("`code list --format md` is a table with one row per code", async () => {
        const json = JSON.parse(
            (await runOmni(["code", "list", "--format", "json"])).stdout,
        ) as unknown[];
        const md = await runOmni(["code", "list", "--format", "md"]);

        expect(md).toHaveSucceeded();
        expect(md.stdout.trim().split("\n")).toHaveLength(json.length + 2);
    });

    it("`code list --kind` filters by error or notice", async () => {
        const kinds = async (kind: string) =>
            new Set(
                (
                    JSON.parse(
                        (
                            await runOmni([
                                "code",
                                "list",
                                "--kind",
                                kind,
                                "--format",
                                "json",
                            ])
                        ).stdout,
                    ) as Array<{ kind: string }>
                ).map((e) => e.kind),
            );

        expect(await kinds("error")).toEqual(new Set(["error"]));
        expect(await kinds("notice")).toEqual(new Set(["notice"]));
    });

    it("`omni code` with no subcommand shows its help", async () => {
        const result = await runOmni(["code"]);

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("explain");
        expect(result).toHaveStderrContaining("list");
    });

    it("`code list --kind` rejects an unknown kind", async () => {
        const result = await runOmni(["code", "list", "--kind", "warning"]);

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("possible values");
    });

    it("`diagnostics list` still lists only error codes", async () => {
        const result = await runOmni([
            "diagnostics",
            "list",
            "--format",
            "json",
        ]);

        expect(result).toHaveSucceeded();
        const codes = (
            JSON.parse(result.stdout) as Array<{ code: string }>
        ).map((e) => e.code);
        expect(codes).toContain("meta.filter_failed");
        expect(codes).not.toContain("sandbox.os_disabled");
    });
});

describe("+notices @cli (projections ledger)", () => {
    it("reports an unreadable ledger as `projections.ledger_unreadable` and carries on", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        ws.write(".omni/sources/projection/links.json", "{ not valid json");

        const result = await runOmni(["projection", "status"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toHaveStderrContaining(
            "warning[projections.ledger_unreadable]",
        );
        expect(result).toHaveStderrContaining("links.json");
    });

    it("`-q` hides it, since it is a plain warning", async () => {
        const ws = makeWorkspace(singleProjectSpec());
        ws.write(".omni/sources/projection/links.json", "{ not valid json");

        const result = await runOmni(["-q", "projection", "status"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.err).not.toContain("projections.ledger_unreadable");
    });
});

describe("+notices @cli (tool runs)", () => {
    it("every notice code raised by tools is registered", async () => {
        for (const code of [
            "runtime.experimental",
            "capabilities.ignored",
            "capabilities.not_enforced",
        ]) {
            const result = await runOmni(["code", "explain", code]);
            expect(result).toHaveSucceeded();
        }
    });
});
