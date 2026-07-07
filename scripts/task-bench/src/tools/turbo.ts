import { readFile, rm } from "node:fs/promises";
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
    hasDaemon: true,
    supportedVersions: ["^2.0.0"],
    description:
        "Vercel Turborepo. Runs a persistent daemon (turbod) to speed up warm runs, toggled per invocation via --daemon/--no-daemon; installed as a workspace devDependency.",

    pinnedVersion: (config) => config.versions.turbo,
    devDependencies: (config) => ({ turbo: config.versions.turbo }),
    setup: async (ctx: GenerationContext) => {
        await ctx.write("turbo.json", turboRootConfig(ctx.model));
    },

    run: (task, ctx) => ({
        file: resolveBin(ctx.rootDir, "turbo"),
        args: [
            "run",
            task,
            "--log-order=stream",
            `--concurrency=${ctx.concurrency}`,
            ctx.daemon ? "--daemon" : "--no-daemon",
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
    stopDaemon: async (ctx: ToolContext) => {
        await execa(resolveBin(ctx.rootDir, "turbo"), ["daemon", "stop"], {
            cwd: ctx.rootDir,
            reject: false,
            stdio: "ignore",
        });
    },
    daemonPids: async (ctx: ToolContext) => {
        if (!ctx.daemon) return [];
        // `turbo daemon status` reports the running daemon, including its pid
        // file. Prefer reading that file; fall back to an inline pid if present.
        const result = await execa(
            resolveBin(ctx.rootDir, "turbo"),
            ["daemon", "status"],
            { cwd: ctx.rootDir, reject: false },
        ).catch(() => null);
        if (!result) return [];
        const text = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;

        const pidFile = text.match(/([^\s"]+\.pid)/i)?.[1];
        if (pidFile) {
            const raw = await readFile(pidFile, "utf8").catch(() => "");
            const filePid = Number.parseInt(raw.trim(), 10);
            if (Number.isInteger(filePid) && filePid > 0) return [filePid];
        }

        const inline = text.match(/\bpid[^\d]*(\d+)/i);
        const pid = inline ? Number(inline[1]) : Number.NaN;
        return Number.isInteger(pid) && pid > 0 ? [pid] : [];
    },
};
