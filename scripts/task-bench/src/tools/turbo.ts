import { existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { join } from "node:path";
import { execa } from "execa";
import type { WorkspaceModel } from "../model";
import {
    type GenerationContext,
    removeDist,
    resolveBin,
    type ToolAdapter,
    type ToolContext,
} from "./types";

/**
 * Locate the actual platform-specific turbo native binary, bypassing the
 * Node.js install shim at `node_modules/.bin/turbo`.
 *
 * The shim uses `child_process.spawn()` (not exec) so it stays alive as a
 * wrapper around the real binary.  When the benchmark runner measures resource
 * usage it only sees `subprocess.pid` — the shim — which does almost nothing.
 * The native binary (a grandchild) is discovered lazily by `refreshTree()`,
 * which takes ~20–50 ms on Linux; for a 50 ms warm run that window is too
 * narrow and the process exits before it is ever sampled.
 *
 * By resolving the binary directly we become the direct parent of the Rust
 * process, so its RSS and CPU are visible from the very first sample.
 *
 * The lookup mirrors the shim's own `getBinaryPath()` logic:
 *   - scoped name first (`@turbo/<os>-<arch>`), then legacy (`turbo-<os>-<arch>`)
 *   - on macOS/Windows ARM, also try the x64 emulation fallback
 */
function resolveTurboBin(rootDir: string): string {
    const os = process.platform === "win32" ? "windows" : process.platform;
    const arch = process.arch === "x64" ? "64" : process.arch;
    const ext = process.platform === "win32" ? ".exe" : "";

    // On macOS and Windows, ARM boxes can run x64 binaries under emulation.
    const arches: string[] =
        process.arch === "arm64" &&
        (process.platform === "darwin" || process.platform === "win32")
            ? [arch, "64"]
            : [arch];

    for (const a of arches) {
        for (const prefix of [`@turbo/${os}-${a}`, `turbo-${os}-${a}`]) {
            const candidate = join(
                rootDir,
                "node_modules",
                prefix,
                "bin",
                `turbo${ext}`,
            );
            if (existsSync(candidate)) return candidate;
        }
    }

    // Fall back to the shim if our path calculation missed something.
    return resolveBin(rootDir, "turbo");
}

export function turboRootConfig(model: WorkspaceModel): string {
    const tasks: Record<string, unknown> = {};
    for (const task of model.projects[0]?.tasks ?? []) {
        tasks[task.name] = {
            dependsOn: task.dependencies,
            outputs: task.outputGlobs,
            inputs: ["package.json", "task.mjs", "src/**"],
        };
    }
    return `${JSON.stringify(
        {
            $schema: "https://turbo.build/schema.json",
            // Turbo 2.x defaults to strict env mode, which would strip the
            // benchmark's execution-marker var. Pass it through (not hashed)
            // so cache-hit verification works without perturbing cache keys.
            globalPassThroughEnv: ["TASK_BENCH_EXEC_LOG"],
            tasks,
        },
        null,
        2,
    )}\n`;
}

export const turboAdapter: ToolAdapter = {
    tool: "turbo",
    supportedVersions: ["^2.0.0"],
    description:
        "Vercel Turborepo. Warm runs are served from a local file-system cache with no persistent daemon (turbod is deprecated for `turbo run` in 2.x); installed as a workspace devDependency.",
    // turbod is no longer used for `turbo run` as of Turborepo 2.x (deprecated,
    // removed in 3.0). hasDaemon is false so the bench runner does not attempt
    // daemon-PID tracking, but stopDaemon is still called before cache wipes to
    // clean up any turbod that may be alive from `turbo watch` or the LSP.
    daemon: {
        hasDaemon: false,
        startMode: "auto",
        stopDaemon: async (ctx: ToolContext) => {
            await execa(resolveBin(ctx.rootDir, "turbo"), ["daemon", "stop"], {
                cwd: ctx.rootDir,
                reject: false,
                stdio: "ignore",
            });
        },
    },

    pinnedVersion: (config) => config.versions.turbo,
    devDependencies: (config) => ({ turbo: config.versions.turbo }),
    setup: async (ctx: GenerationContext) => {
        await ctx.write("turbo.json", turboRootConfig(ctx.model));
    },

    run: (task, ctx) => ({
        // Use the native binary directly, not the Node.js install shim.
        // See resolveTurboBin() for the full rationale.
        file: resolveTurboBin(ctx.rootDir),
        args: [
            "run",
            task,
            "--log-order=stream",
            `--concurrency=${ctx.concurrency}`,
            // --daemon / --no-daemon are deprecated in 2.x and removed in 3.0.
        ],
    }),
    env: () => ({}),
    clearCaches: async (ctx: ToolContext) => {
        await removeDist(ctx);
        await rm(join(ctx.rootDir, ".turbo"), {
            recursive: true,
            force: true,
        });
        await rm(join(ctx.rootDir, "node_modules", ".cache", "turbo"), {
            recursive: true,
            force: true,
        });
    },
};
