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
        project: "GgxpjFjo7z4kxo2kFvrKDxibGJnrYDyg1pizjk6HMdmi",
        workspace: "9gjYApnKLQKtQrrq4Ttj3eevrHXPSnXK39UD7k3PCQZK",
    },
    {
        label: "dependency explicit `up#x` then `^x`",
        def: { dependencies: ["up#x", "^x"] },
        project: "AAVHWafQ6CSqWKbEELaKvACq9NwTXugjGUyBUchUGJwN",
        workspace: "9UN25umWEwte9ChNax2zTSdkNNKYpdDsxRUaPuTPA8Tn",
    },
    {
        label: "explicit-only duplicate dependency",
        def: { dependencies: ["up#x", "up#x"] },
        project: "6vCAbFTWw98rVhuFtcph8b8Gjg6bvQik93tSeKrThqgi",
        workspace: "A9EQPDABytLmgHYkga3M7a17DGGJEsry9AZ8D6nwtB7m",
    },
    {
        label: "single explicit dependency",
        def: { dependencies: ["up#x"] },
        project: "3VmQ3bBwp5erZTkeapYhxtcwNmSrY2hAApWT9jiC2usx",
        workspace: "7cYZu2y4kPoSj4sScxRCcJEUtftPnyoN7P3nYjgvFNGF",
    },
    {
        label: "`with` explicit `up#x` and `^x`",
        def: { with: ["up#x", "^x"] },
        project: "2nSoFGnHUtFS26MqNzhLnqdKbrAM36WLEusXgtWjZ6Uz",
        workspace: "J5TB3mzb6vcVcSyeJfwEs8AjVf4mFhDyjiUnnXN8Rra1",
    },
    {
        label: "`with: up#x` plus dependency `^x`",
        def: { with: ["up#x"], dependencies: ["^x"] },
        project: null,
        workspace: "t5Kpka3n3K6EdUPdy6BCz8s6fxWypxeYN5aUJ45x42s",
    },
    {
        label: "`with: ^x` plus dependency `up#x`",
        def: { with: ["^x"], dependencies: ["up#x"] },
        project: null,
        workspace: "xBEomZdXo49qynmyskn2SK11AAbRmrjbTww5F7G7SvZ",
    },
    {
        label: "`with: ^x` plus dependency `^x`",
        def: { with: ["^x"], dependencies: ["^x"] },
        project: null,
        workspace: "CxggMpxnRXRMg3u71hstTRQDFA9vvaWJhX8tf7oPjQMC",
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
