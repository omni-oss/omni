/**
 * Contained script reads (`enable_experimental.contained_reads`).
 *
 * With capabilities enforcement and `contained_reads` both on, the two file
 * read routes a generator script uses (`ctx.sys.fs.readFileAsString` and
 * `readFileAsBytes`) read workspace files through the host's contained input
 * view: symlinks are refused, size limits are enforced before allocation, and
 * a platform that cannot contain the read fails it. Without the key nothing
 * changes.
 *
 * Pinned to `crates/omni_contained_read`, `crates/omni_containment_configurations`
 * and `crates/omni_generator/src/script_runner.rs`. Requires a JS runtime
 * (node/bun/deno) on PATH.
 */

import { spawnSync } from "node:child_process";
import { symlinkSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
    makeWorkspace,
    runOmni,
    type Workspace,
    type WorkspaceSpec,
} from "@/harness";

function runtimeAvailable(bin: string): boolean {
    try {
        return spawnSync(bin, ["--version"], { stdio: "ignore" }).status === 0;
    } catch {
        return false;
    }
}

const HAS_RUNTIME = ["deno", "node", "bun"].some(runtimeAvailable);

/** The code a refused symlink read carries on this platform. */
const SYMLINK_CODE =
    process.platform === "win32"
        ? "input_view.reparse_point"
        : process.platform === "darwin"
          ? "input_view.containment_unsupported"
          : "input_view.symlink";

/** macOS cannot contain a read (D5): even a plain read fails closed. */
const CONTAINMENT_SUPPORTED = process.platform !== "darwin";

const READ_ALL = [
    { access: "allow", domain: "fs.read", patterns: ["@workspace/**"] },
    { access: "allow", domain: "fs.write", patterns: ["@project/**"] },
];

interface Options {
    /** Path (relative to the output dir) the script reads. */
    read: string;
    experimental: unknown;
    containment?: Record<string, unknown>;
}

function spec(opts: Options): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
            enable_experimental: opts.experimental,
            ...(opts.containment ? { containment: opts.containment } : {}),
        },
        projects: {
            "generators/capgen/generator.omni.yaml": {
                name: "capgen",
                description: "contained reads",
                capabilities: { rules: READ_ALL },
                actions: [
                    {
                        type: "run-javascript",
                        script: "gen.mjs",
                        data: { read: opts.read },
                    },
                ],
            },
        },
        files: {
            ".omni/sources/generator/.keep": "",
            "real.txt": "0123456789",
            // The script is deliberately not catching the read error, so a
            // refused read fails the run.
            "generators/capgen/gen.mjs": `export default async function (ctx) {
                const text = await ctx.sys.fs.readFileAsString(ctx.data.read);
                await ctx.sys.fs.writeStringToFile("copy.txt", text);
            }`,
        },
    };
}

function run(ws: Workspace) {
    return runOmni(
        [
            "generator",
            "run",
            "-n",
            "capgen",
            "-o",
            "out",
            "--use-defaults",
            "--save-session=false",
        ],
        { cwd: ws.cwd },
    );
}

const ON = { capabilities: true, contained_reads: true };

describe.skipIf(!HAS_RUNTIME)(
    "+generator @e2e (contained script reads)",
    { tags: ["generator"], timeout: 60_000 },
    () => {
        it("reads a regular workspace file through the view", async () => {
            const ws = makeWorkspace(
                spec({ read: "../real.txt", experimental: ON }),
            );

            const result = await run(ws);

            // macOS fails the read closed instead (D5, no fallback).
            expect(result.exitCode === 0).toBe(CONTAINMENT_SUPPORTED);
            expect(
                result.err.includes("input_view.containment_unsupported"),
            ).toBe(!CONTAINMENT_SUPPORTED);
            expect(
                ws.exists("out/copy.txt") ? ws.read("out/copy.txt") : null,
            ).toBe(CONTAINMENT_SUPPORTED ? "0123456789" : null);
        });

        it("refuses a symlink with its input_view code and a failing exit status", async () => {
            const ws = makeWorkspace(
                spec({ read: "../link.txt", experimental: ON }),
            );
            symlinkSync(ws.path("real.txt"), ws.path("link.txt"));

            const result = await run(ws);

            expect(result).toHaveFailed();
            expect(result.err).toContain(SYMLINK_CODE);
            expect(ws.exists("out/copy.txt")).toBe(false);
        });

        it("does not change anything without the key (symlinks still read)", async () => {
            const ws = makeWorkspace(
                spec({
                    read: "../link.txt",
                    experimental: { capabilities: true },
                }),
            );
            symlinkSync(ws.path("real.txt"), ws.path("link.txt"));

            const result = await run(ws);

            expect(result).toHaveSucceeded();
            expect(ws.read("out/copy.txt")).toBe("0123456789");
        });

        it("ignores the key without capabilities enforcement and says so", async () => {
            const ws = makeWorkspace(
                spec({
                    read: "../link.txt",
                    experimental: { contained_reads: true },
                }),
            );
            symlinkSync(ws.path("real.txt"), ws.path("link.txt"));

            const result = await run(ws);

            expect(result).toHaveSucceeded();
            expect(result.err).toContain("`contained_reads` is enabled");
        });

        it("enforces the configured file limit", async () => {
            const ws = makeWorkspace(
                spec({
                    read: "../real.txt",
                    experimental: ON,
                    containment: {
                        limits: {
                            fs: {
                                read: { max_file_bytes: 4, max_total_bytes: 4 },
                            },
                        },
                    },
                }),
            );

            const result = await run(ws);

            expect(result).toHaveFailed();
            expect(result.err).toContain(
                CONTAINMENT_SUPPORTED
                    ? "input_view.too_large"
                    : "input_view.containment_unsupported",
            );
        });

        it("rejects a limit of zero or above the host ceiling when the workspace loads", async () => {
            for (const value of [0, 999_999_999_999]) {
                const ws = makeWorkspace(
                    spec({
                        read: "../real.txt",
                        experimental: ON,
                        containment: {
                            limits: { fs: { read: { max_file_bytes: value } } },
                        },
                    }),
                );

                const result = await run(ws);

                expect(result).toHaveFailed();
                expect(result.err).toContain("max_file_bytes");
            }
        });

        it("registers the new codes", async () => {
            for (const code of [
                "capabilities.contained_reads_ignored",
                "input_view.symlink",
                "input_view.too_large",
                "input_view.containment_unsupported",
            ]) {
                const result = await runOmni(["code", "explain", code]);
                expect(result).toHaveSucceeded();
            }
        });
    },
);
