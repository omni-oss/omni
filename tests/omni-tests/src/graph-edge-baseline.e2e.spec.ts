/**
 * RFC 0024 F03.4: digest baselines for reference shapes whose duplicate
 * handling the alias graph rework touches. The literals were captured from the
 * pre-alias base (c7aa81fc) and must not change: the `has_connected_node` fix
 * is behavior-preserving for graphs built from configuration.
 *
 * Re-pinned once, for the cache key version and effective-task leaves (RFC 0024
 * F06.3). That is the only change: the previous literals reproduce exactly with
 * those two leaves switched off, so the alias rework is still behavior
 * preserving. The `pins the pre-alias digest` titles are kept for that reason.
 * Previous values (project, workspace) in the order of `shapes`:
 * 3M9DWKis..., ENLb7XWm...; 4ZiG86Vx..., GZWXMkWs...; 8gHuiuWe..., Gp2MLnbo...;
 * ERnE44bi..., 42fp2pps...; EGPmChrH..., 7uEjX3hs...; (null), GFCktRQT...;
 * (null), 7xzGwFMZ...; (null), 5YWMKEpU....
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni } from "@/harness";

function shape(def: Record<string, unknown>) {
    return makeWorkspace({
        workspace: { projects: ["**"] },
        projects: {
            up: { name: "up", tasks: { x: 'echo "x"' } },
            down: {
                name: "down",
                dependencies: ["up"],
                tasks: { t: { exec: 'echo "t"', ...def } as never },
            },
        },
    });
}

async function digest(cwd: string, args: string[]) {
    const result = await runOmni(["hash", "-r", ...args], { cwd });
    return result;
}

const shapes: Array<{
    label: string;
    def: Record<string, unknown>;
    project: string | null;
    workspace: string;
}> = [
    {
        label: "dependency `^x` then explicit `up#x`",
        def: { dependencies: ["^x", "up#x"] },
        project: "8gCiuzZVYfT89qp8a7ckP1DVr2Y9H2h3yGDGZcnELMPT",
        workspace: "2ARH9nBtbPZxANVFNAxTQTuv494B1EKNKHX1iuTCNabd",
    },
    {
        label: "dependency explicit `up#x` then `^x`",
        def: { dependencies: ["up#x", "^x"] },
        project: "3KepXMKVJiUata82zz1EAzYB95TgQUgKtakbP5fmrMhk",
        workspace: "3QqiQhdi62qZaQoApnZvHn7ZiWbVZ2vwh9DqeE2NQjXi",
    },
    {
        label: "explicit-only duplicate dependency",
        def: { dependencies: ["up#x", "up#x"] },
        project: "BgBQL16cKSGD4vvY8Z6DHP9PwUFScCWDB3wVtmk66ekU",
        workspace: "2viq91uv6X6tY7cRMhgjo8kU81siq2S8kQx447QhgVVo",
    },
    {
        label: "single explicit dependency",
        def: { dependencies: ["up#x"] },
        project: "4SsEjyFtMTugs9k9oUbTR2uBLA5E1E381o1d7F8uvazg",
        workspace: "9LyZrTi5gWJ7tUFnzrDUwv8CL8T1sPQjz2RajhAzEBq8",
    },
    {
        label: "`with` explicit `up#x` and `^x`",
        def: { with: ["up#x", "^x"] },
        project: "9ssx3zcUAQ8XgEUwN6fpXGecXTNAnV1f7CM29bvUWyXY",
        workspace: "2nhR1xAg9x72VAWEgWcgiuHjbMR9CSaii6oQvnEqnQBJ",
    },
    {
        label: "`with: up#x` plus dependency `^x`",
        def: { with: ["up#x"], dependencies: ["^x"] },
        project: null,
        workspace: "8XKHCHSvd9T8HkVZUUkhUcFNF91rfPPYkL2jC2b1iMhh",
    },
    {
        label: "`with: ^x` plus dependency `up#x`",
        def: { with: ["^x"], dependencies: ["up#x"] },
        project: null,
        workspace: "5KvZcyMXZAcNYi9sekFi3eD1hCTiyxZYJh3NZ7J1S1hZ",
    },
    {
        label: "`with: ^x` plus dependency `^x`",
        def: { with: ["^x"], dependencies: ["^x"] },
        project: null,
        workspace: "HJyo22JZGLi8oCPP7HEPqxetpafZxCekepK7vpJ8XwQd",
    },
];

describe("+hashing @e2e (duplicate-reference digest baselines)", () => {
    for (const { label, def, project, workspace } of shapes) {
        it(`pins the pre-alias digest for ${label}`, async () => {
            const ws = shape(def);
            const proj = await digest(ws.cwd, ["project", "down", "-t", "t"]);
            if (project === null) {
                // The project aggregate rejects a task that is both a sibling
                // and a dependent of one target, before and after the rework.
                expect(proj).not.toHaveSucceeded();
                expect(proj.stderr).toContain("task.sibling_cycle");
            } else {
                expect(proj).toHaveSucceeded();
                expect(proj.stdout.trim()).toBe(project);
            }
            const all = await digest(ws.cwd, ["workspace"]);
            expect(all).toHaveSucceeded();
            expect(all.stdout.trim()).toBe(workspace);
        });
    }
});
