/**
 * `omni project` - listing projects and printing their merged configuration.
 * Pinned to `crates/omni_cli_core/src/commands/project.rs`.
 *
 * Both subcommands load the workspace's project configurations, which logs an
 * `INFO Loaded project configurations` line to stdout at the default level. We
 * pass `-r/--raw` (which swaps in a no-op tracing subscriber) whenever a test
 * needs clean, parseable output; the default-level noise is asserted separately
 * by the `-r` test itself.
 *
 * Project discovery order is not guaranteed, so list assertions compare the set
 * of names rather than their order.
 */

import { parse as parseToml } from "smol-toml";
import { describe, expect, it } from "vitest";
import { parse as parseYaml } from "yaml";

import {
    extendsSpec,
    lines,
    makeWorkspace,
    mixedProjectConfigsSpec,
    runOmni,
} from "@/harness";

const NAMES = ["alpha", "beta", "gamma"] as const;

/** A workspace with three sibling projects named after {@link NAMES}. */
function projectsWorkspace() {
    return makeWorkspace({
        workspace: { projects: ["**"] },
        projects: Object.fromEntries(
            NAMES.map((name) => [
                name,
                { name, tasks: { build: `echo "${name}"` } },
            ]),
        ),
    });
}

describe("+project @e2e (list)", () => {
    it("`project list` prints one project name per line", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "list"], { cwd: ws.cwd });

        expect(result).toHaveSucceeded();
        // Every project shows up as its own line (alongside the INFO log line).
        for (const name of NAMES) {
            expect(lines(result.out)).toContain(name);
        }
    });

    it("`project list -r/--raw` suppresses the load log", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "list", "-r"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result.out).not.toContain("Loaded project configurations");
        // With logs gone, stdout is exactly the project names.
        expect([...lines(result.out)].sort()).toEqual([...NAMES].sort());
    });
});

describe("+project @output (list serialization)", () => {
    it("`project list -f json` serializes the name list as a JSON array", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "list", "-r", "-f", "json"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        const parsed = JSON.parse(result.stdout) as string[];
        expect([...parsed].sort()).toEqual([...NAMES].sort());
    });

    it("`project list -f yaml` serializes the name list as a YAML sequence", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "list", "-r", "-f", "yaml"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        const parsed = parseYaml(result.stdout) as string[];
        expect([...parsed].sort()).toEqual([...NAMES].sort());
    });

    it("`project list -f toml` wraps the names under a `projects` key", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "list", "-r", "-f", "toml"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        const parsed = parseToml(result.stdout) as { projects: string[] };
        expect([...parsed.projects].sort()).toEqual([...NAMES].sort());
    });
});

describe("+project @output (print-config)", () => {
    it("`project print-config <name>` prints the config as JSON by default", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(
            ["project", "print-config", "alpha", "-r"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        const parsed = JSON.parse(result.stdout) as Record<string, unknown>;
        expect(parsed.name).toBe("alpha");
        expect(parsed.tasks).toMatchObject({ build: 'echo "alpha"' });
    });

    it("print-config -f yaml|toml serializes in the requested format", async () => {
        const ws = projectsWorkspace();

        const yaml = await runOmni(
            ["project", "print-config", "alpha", "-r", "-f", "yaml"],
            {
                cwd: ws.cwd,
            },
        );
        const toml = await runOmni(
            ["project", "print-config", "alpha", "-r", "-f", "toml"],
            {
                cwd: ws.cwd,
            },
        );

        expect(yaml).toHaveSucceeded();
        expect((parseYaml(yaml.stdout) as { name: string }).name).toBe("alpha");

        expect(toml).toHaveSucceeded();
        expect((parseToml(toml.stdout) as { name: string }).name).toBe("alpha");
    });

    it("print-config -r/--raw prints only the configuration, no logs", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(
            ["project", "print-config", "alpha", "-r"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        expect(result.out).not.toContain("Loaded project configurations");
        // Raw output starts immediately with the JSON object.
        expect(result.stdout.trimStart().startsWith("{")).toBe(true);
    });

    it("print-config shows the merged config (base + extends applied)", async () => {
        const ws = makeWorkspace(extendsSpec());

        const result = await runOmni(
            ["project", "print-config", "child", "-r"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        const parsed = JSON.parse(result.stdout) as {
            tasks: Record<string, string>;
        };
        // `own` is declared on the child; `from-base` is merged in from the base.
        expect(parsed.tasks).toHaveProperty("own");
        expect(parsed.tasks).toHaveProperty("from-base");
    });
});

describe("+project @output (raw + format combinations)", () => {
    it("`project list -r -f json` is a clean JSON array of names", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "list", "-r", "-f", "json"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        // `-r` strips the load log, so stdout is exactly the serialized array.
        expect(result.out).not.toContain("Loaded project configurations");
        const parsed = JSON.parse(result.stdout) as string[];
        expect(Array.isArray(parsed)).toBe(true);
        expect([...parsed].sort()).toEqual([...NAMES].sort());
    });

    it("`project print-config -r -f yaml <name>` is clean YAML", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(
            ["project", "print-config", "-r", "-f", "yaml", "alpha"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        // `-r` suppresses the load log, leaving only the YAML document.
        expect(result.out).not.toContain("Loaded project configurations");
        const parsed = parseYaml(result.stdout) as {
            name: string;
            tasks: Record<string, string>;
        };
        expect(parsed.name).toBe("alpha");
        expect(parsed.tasks).toMatchObject({ build: 'echo "alpha"' });
    });

    it("`project print-config -f toml <name>` round-trips the merged config", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(
            ["project", "print-config", "-f", "toml", "alpha"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        // Without `-r`, an `INFO Loaded project configurations` log precedes the
        // document on stdout; drop those tracing lines, then the rest is the
        // merged config serialized as TOML.
        const tomlBody = lines(result.out)
            .filter((line) => !/\b(INFO|WARN|ERROR|DEBUG|TRACE)\b/.test(line))
            .join("\n");
        const parsed = parseToml(tomlBody) as {
            name: string;
            tasks: Record<string, string>;
        };
        expect(parsed.name).toBe("alpha");
        expect(parsed.tasks).toMatchObject({ build: 'echo "alpha"' });
    });
});

describe("+project @exitcode (errors)", () => {
    it("print-config with an unknown project logs a clear error", async () => {
        const ws = projectsWorkspace();

        const result = await runOmni(["project", "print-config", "nope"], {
            cwd: ws.cwd,
        });

        // The command logs the error and still exits 0 (no config is printed).
        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("No project named 'nope' found");
        expect(result.stdout).not.toContain("{");
    });
});

/** A workspace with one config of every kind (see the fixture). */
function mixedWorkspace() {
    return makeWorkspace(mixedProjectConfigsSpec());
}

interface ListEntry {
    name: string;
    path: string;
    base: boolean;
    discovered: boolean;
}

const ALL_ENTRIES: ListEntry[] = [
    {
        name: "app",
        path: "apps/app/project.omni.yaml",
        base: false,
        discovered: true,
    },
    {
        name: "lib",
        path: "apps/lib/project.omni.yaml",
        base: false,
        discovered: true,
    },
    {
        name: "shared-base",
        path: "shared/base.omni.yaml",
        base: true,
        discovered: false,
    },
    {
        name: "tpl",
        path: "apps/tpl/project.omni.yaml",
        base: true,
        discovered: true,
    },
];

describe("+project @e2e (list visibility)", () => {
    it("`project list` lists only runnable projects, sorted", async () => {
        const ws = mixedWorkspace();

        const result = await runOmni(["project", "list", "-r"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(lines(result.out)).toEqual(["app", "lib"]);
    });

    it("visibility flags add base and extends-only configs", async () => {
        const ws = mixedWorkspace();

        const names = async (flag: string) => {
            const result = await runOmni(
                ["project", "list", "-r", "-f", "json", flag],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            return (JSON.parse(result.stdout) as ListEntry[]).map(
                (e) => e.name,
            );
        };

        expect(await names("--include-base")).toEqual([
            "app",
            "lib",
            "shared-base",
            "tpl",
        ]);
        expect(await names("--include-extended")).toEqual([
            "app",
            "lib",
            "shared-base",
        ]);
        expect(await names("--all")).toEqual([
            "app",
            "lib",
            "shared-base",
            "tpl",
        ]);
    });

    it("`--all` combined with an include flag is the same as `--all`", async () => {
        const ws = mixedWorkspace();

        const result = await runOmni(
            ["project", "list", "-r", "-f", "json", "--all", "--include-base"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(JSON.parse(result.stdout)).toEqual(ALL_ENTRIES);
    });

    it("without a visibility flag the output keeps the name-only shape", async () => {
        const ws = mixedWorkspace();

        const json = await runOmni(["project", "list", "-r", "-f", "json"], {
            cwd: ws.cwd,
        });
        const toml = await runOmni(["project", "list", "-r", "-f", "toml"], {
            cwd: ws.cwd,
        });

        expect(json).toHaveSucceeded();
        expect(JSON.parse(json.stdout)).toEqual(["app", "lib"]);
        expect(toml).toHaveSucceeded();
        expect(parseToml(toml.stdout)).toEqual({ projects: ["app", "lib"] });
    });
});

describe("+project @output (annotated list)", () => {
    it("text output aligns name, tags and path columns", async () => {
        const ws = mixedWorkspace();

        const result = await runOmni(["project", "list", "-r", "--all"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(lines(result.out)).toEqual([
            "app          project        apps/app/project.omni.yaml",
            "lib          project        apps/lib/project.omni.yaml",
            "shared-base  base,extended  shared/base.omni.yaml",
            "tpl          base           apps/tpl/project.omni.yaml",
        ]);
    });

    it("json and yaml serialize entry objects", async () => {
        const ws = mixedWorkspace();

        const json = await runOmni(
            ["project", "list", "-r", "-f", "json", "--all"],
            { cwd: ws.cwd },
        );
        const yaml = await runOmni(
            ["project", "list", "-r", "-f", "yaml", "--all"],
            { cwd: ws.cwd },
        );

        expect(json).toHaveSucceeded();
        expect(JSON.parse(json.stdout)).toEqual(ALL_ENTRIES);
        expect(yaml).toHaveSucceeded();
        expect(parseYaml(yaml.stdout)).toEqual(ALL_ENTRIES);
    });

    it("toml serializes entries as `[[projects]]` tables", async () => {
        const ws = mixedWorkspace();

        const result = await runOmni(
            ["project", "list", "-r", "-f", "toml", "--all"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("[[projects]]");
        expect(parseToml(result.stdout)).toEqual({ projects: ALL_ENTRIES });
    });
});

describe("+project @config (print-config selection)", () => {
    it("`print-config --path` resolves a relative path from a subdirectory", async () => {
        const ws = mixedWorkspace();

        const result = await runOmni(
            [
                "project",
                "print-config",
                "-r",
                "--path",
                "tpl/project.omni.yaml",
            ],
            { cwd: ws.path("apps") },
        );

        expect(result).toHaveSucceeded();
        const parsed = JSON.parse(result.stdout) as {
            name: string;
            base: boolean;
        };
        expect(parsed.name).toBe("tpl");
        expect(parsed.base).toBe(true);
    });

    it("`print-config --path` selects an extends-only config", async () => {
        const ws = mixedWorkspace();

        const result = await runOmni(
            [
                "project",
                "print-config",
                "-r",
                "--path",
                "shared/base.omni.yaml",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect((JSON.parse(result.stdout) as { name: string }).name).toBe(
            "shared-base",
        );
    });

    it("`print-config --path` to a file that is not loaded logs an error", async () => {
        const ws = mixedWorkspace();
        ws.write("stray.omni.yaml", { name: "stray" });

        const result = await runOmni(
            ["project", "print-config", "--path", "stray.omni.yaml"],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("no loaded project configuration");
        expect(result.stdout).not.toContain("{");
    });

    it("print-config requires exactly one of a name or `--path`", async () => {
        const ws = mixedWorkspace();

        const both = await runOmni(
            [
                "project",
                "print-config",
                "app",
                "--path",
                "apps/app/project.omni.yaml",
            ],
            { cwd: ws.cwd },
        );
        const neither = await runOmni(["project", "print-config"], {
            cwd: ws.cwd,
        });

        expect(both).toHaveExitCode(2);
        expect(neither).toHaveExitCode(2);
    });

    it("print-config prefers the runnable project over a same-named template", async () => {
        const ws = mixedWorkspace();
        ws.write("apps/a-app-tpl/project.omni.yaml", {
            name: "app",
            base: true,
        });

        const result = await runOmni(["project", "print-config", "-r", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect((JSON.parse(result.stdout) as { base: boolean }).base).toBe(
            false,
        );
        expect(result.stderr).not.toContain("warning:");
    });

    it("print-config warns when several templates share the name", async () => {
        const ws = mixedWorkspace();
        ws.write("apps/b-tpl/project.omni.yaml", { name: "tpl", base: true });

        const result = await runOmni(["project", "print-config", "-r", "tpl"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(JSON.parse(result.stdout)).toMatchObject({ name: "tpl" });
        expect(result).toHaveStderrContaining("warning:");
        expect(result).toHaveStderrContaining("apps/tpl/project.omni.yaml");
        expect(result).toHaveStderrContaining("--path");
    });

    it("print-config errors when two runnable projects share the name", async () => {
        const ws = mixedWorkspace();
        ws.write("apps/app2/project.omni.yaml", { name: "app" });

        const result = await runOmni(["project", "print-config", "app"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("duplicate project name");
        expect(result).toOutputContaining("apps/app/project.omni.yaml");
        expect(result).toOutputContaining("apps/app2/project.omni.yaml");
        expect(result.stdout).not.toContain("{");
    });
});
