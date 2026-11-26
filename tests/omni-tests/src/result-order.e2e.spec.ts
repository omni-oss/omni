/**
 * `--result` lists entries sorted by (project, task), the same on every run.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni } from "@/harness";

const PROJECTS = [
    "zeta",
    "alpha",
    "mu",
    "beta",
    "omega",
    "gamma",
    "kappa",
    "delta",
    "sigma",
    "epsilon",
    "theta",
    "iota",
    "lambda",
    "eta",
    "rho",
    "pi",
];

const makeMultiProjectWorkspace = () =>
    makeWorkspace({
        workspace: { projects: ["**"] },
        projects: Object.fromEntries(
            PROJECTS.map((name) => [
                name,
                {
                    name,
                    tasks: { build: "echo build", lint: "echo lint" },
                },
            ]),
        ),
    });

type ResultEntry = { task: { project_name: string; task_name: string } };

const keysOf = (json: string): [string, string][] =>
    (JSON.parse(json) as ResultEntry[]).map((r) => [
        r.task.project_name,
        r.task.task_name,
    ]);

const compareKeys = (a: [string, string], b: [string, string]) =>
    a[0] === b[0]
        ? a[1] < b[1]
            ? -1
            : a[1] > b[1]
              ? 1
              : 0
        : a[0] < b[0]
          ? -1
          : 1;

describe("+result-order @output (result file order)", () => {
    it("`run --result` is sorted by project then task and stable across runs", async () => {
        const ws = makeMultiProjectWorkspace();
        const runs: [string, string][][] = [];

        for (const file of ["first.json", "second.json"]) {
            const result = await runOmni(
                ["run", "build", "lint", "--result", file],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            runs.push(keysOf(ws.read(file)));
        }

        expect(runs[0]).toHaveLength(PROJECTS.length * 2);
        const first = runs[0] ?? [];
        expect(first).toEqual([...first].sort(compareKeys));
        expect(runs[1]).toEqual(first);
    });

    it("`exec --result` is sorted by project and stable across runs (the ad-hoc task name is random per run)", async () => {
        const ws = makeMultiProjectWorkspace();
        const runs: [string, string][][] = [];

        for (const file of ["first.json", "second.json"]) {
            const result = await runOmni(
                ["exec", "--result", file, "--", "echo", "hi"],
                { cwd: ws.cwd },
            );
            expect(result).toHaveSucceeded();
            runs.push(keysOf(ws.read(file)).map(([project]) => [project, ""]));
        }

        expect(runs[0]).toHaveLength(PROJECTS.length);
        const first = runs[0] ?? [];
        expect(first).toEqual([...first].sort(compareKeys));
        expect(runs[1]).toEqual(first);
    });
});
