/**
 * RFC 0024 F03.4: digest baselines for reference shapes whose duplicate
 * handling the alias graph rework touches. The literals were captured from the
 * pre-alias base (c7aa81fc) and must not change: the `has_connected_node` fix
 * is behavior-preserving for graphs built from configuration.
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
        project: "3M9DWKisgNVMUb8rJAUCqLZEWJZg8sTBjDYwLYygivps",
        workspace: "ENLb7XWmLK7UH8uFAiZ6NoJuYDtuMte6NrWF7wV13N7v",
    },
    {
        label: "dependency explicit `up#x` then `^x`",
        def: { dependencies: ["up#x", "^x"] },
        project: "4ZiG86VxAd3ymSbNamfCdA3RM1a51HSqTUzpxpXWSzYt",
        workspace: "GZWXMkWsUy648MBEu1mhKAJ55tiT9x7mQMyuNxq4qrSK",
    },
    {
        label: "explicit-only duplicate dependency",
        def: { dependencies: ["up#x", "up#x"] },
        project: "8gHuiuWedB8fCreepQsCLQAc3176nudX8XfgkJafC5S1",
        workspace: "Gp2MLnbo8fB9v6PUuqH63J3eWWJciVFTnqNCCmjG33x6",
    },
    {
        label: "single explicit dependency",
        def: { dependencies: ["up#x"] },
        project: "ERnE44biwYhumsDQVrntGrYd8PyU4XZVPEQVK8CXGhrE",
        workspace: "42fp2ppsjuJigQbPXF3kAWo2YjA1rBqMVu2y9TqfML2j",
    },
    {
        label: "`with` explicit `up#x` and `^x`",
        def: { with: ["up#x", "^x"] },
        project: "EGPmChrHHRJoGsNjDoWsakTZBtSiwBdaaMqFp5L5z4Na",
        workspace: "7uEjX3hsz65VjvsW9CLnyZmgHPNVmyyzBdde5nFMjX8V",
    },
    {
        label: "`with: up#x` plus dependency `^x`",
        def: { with: ["up#x"], dependencies: ["^x"] },
        project: null,
        workspace: "GFCktRQT6R7fiUCLyHRjVGNT7kDWNuGQAnAoHNwhCNWK",
    },
    {
        label: "`with: ^x` plus dependency `up#x`",
        def: { with: ["^x"], dependencies: ["up#x"] },
        project: null,
        workspace: "7xzGwFMZ4Gjd5QRMiuFTDHPxjA8WcgJfDkyuQkeWLsgu",
    },
    {
        label: "`with: ^x` plus dependency `^x`",
        def: { with: ["^x"], dependencies: ["^x"] },
        project: null,
        workspace: "5YWMKEpUF1sS8jJKwmPRsCibMCu4x5AxCARYT4iEYdHN",
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
