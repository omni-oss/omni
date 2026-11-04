/**
 * The `crate` generator (`generators/crate`) scaffolds crates in the shape
 * RFC 0023 requires: `tracing` only, no `trace` wrapper, no `log`, and no
 * `enable-tracing` feature. (The workspace-level check that a generated crate
 * builds and passes the `disallowed_macros` lint is `cargo clippy` in CI.)
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

import { describe, expect, it } from "vitest";

import { findRepoRoot, makeWorkspace, runOmni } from "@/harness";

/** Every file under `generators/crate`, keyed by its path in a workspace. */
function crateGeneratorFiles(): Record<string, string> {
    const root = findRepoRoot();
    const base = join(root, "generators", "crate");
    const out: Record<string, string> = {};
    const walk = (dir: string) => {
        for (const entry of readdirSync(dir)) {
            const abs = join(dir, entry);
            if (statSync(abs).isDirectory()) walk(abs);
            else
                out[
                    `generators/crate/${relative(base, abs).replaceAll("\\", "/")}`
                ] = readFileSync(abs, "utf8");
        }
    };
    walk(base);
    return out;
}

function generate(crateType: "lib" | "bin", useTracing: boolean) {
    const ws = makeWorkspace({
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        files: {
            ...crateGeneratorFiles(),
            "Cargo.toml": "[workspace]\n# @anchor:new-crate\n",
            "cog.toml": "# @anchor:new-rust-package\n",
        },
    });
    return ws;
}

describe(
    "+generator @e2e (crate generator, RFC 0023)",
    { tags: ["generator"] },
    () => {
        it.each([
            ["lib", true],
            ["bin", true],
            ["lib", false],
        ] as const)(
            "a %s crate (use_tracing=%s) has no `trace`, `log` or `enable-tracing`",
            async (crateType, useTracing) => {
                const ws = generate(crateType, useTracing);

                const result = await runOmni(
                    [
                        "generator",
                        "run",
                        "-n",
                        "crate",
                        "-o",
                        "crates/sample",
                        "--use-defaults",
                        "--overwrite",
                        "always",
                        "-v",
                        `crate_type=${crateType}`,
                        "-v",
                        "version=workspace",
                        "-v",
                        `use_tracing=${useTracing}`,
                        "-v",
                        "add_error_type=false",
                    ],
                    { cwd: ws.cwd },
                );

                const cargo = ws.read("crates/sample/Cargo.toml");
                const project = ws.read("crates/sample/project.omni.yaml");

                expect(cargo).not.toMatch(/^trace\s*=/m);
                expect(cargo).not.toMatch(/^log\s*=/m);
                expect(cargo).not.toContain("enable-tracing");
                expect(cargo).not.toContain("[features]");
                expect(project).not.toMatch(/-\s*trace\b/);
                expect(project).not.toContain("dependencies:");

                if (useTracing) {
                    expect(cargo).toMatch(
                        /^tracing = \{ workspace = true \}$/m,
                    );
                } else {
                    expect(cargo).not.toMatch(/^tracing\s*=/m);
                }
            },
        );

        it("a bin crate's sample main may print, so it passes the output lint", async () => {
            const ws = generate("bin", true);

            await runOmni(
                [
                    "generator",
                    "run",
                    "-n",
                    "crate",
                    "-o",
                    "crates/sample",
                    "--use-defaults",
                    "--overwrite",
                    "always",
                    "-v",
                    "crate_type=bin",
                    "-v",
                    "version=workspace",
                    "-v",
                    "add_error_type=false",
                ],
                { cwd: ws.cwd },
            );

            expect(ws.read("crates/sample/src/main.rs")).toContain(
                "#![allow(clippy::disallowed_macros)]",
            );
        });
    },
);
