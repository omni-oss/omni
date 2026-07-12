import { execa } from "execa";
import type { ProjectModel, WorkspaceModel } from "../model";
import {
    type GenerationContext,
    removeDist,
    resolveBin,
    type ToolAdapter,
    type ToolContext,
} from "./types";

export function nxRootConfig(model: WorkspaceModel): string {
    const targetDefaults: Record<string, unknown> = {};
    for (const task of model.projects[0]?.tasks ?? []) {
        targetDefaults[task.name] = {
            dependsOn: task.dependencies,
            cache: true,
            outputs: task.outputGlobs.map((glob) => `{projectRoot}/${glob}`),
            inputs: [
                "{projectRoot}/package.json",
                "{projectRoot}/task.mjs",
                "{projectRoot}/src/**/*",
            ],
        };
    }
    return `${JSON.stringify(
        {
            $schema: "./node_modules/nx/schemas/nx-schema.json",
            targetDefaults,
        },
        null,
        2,
    )}\n`;
}

export function nxProjectConfig(project: ProjectModel): string {
    const targets: Record<string, unknown> = {};
    for (const task of project.tasks) {
        targets[task.name] = {
            executor: "nx:run-commands",
            options: {
                command: `node ./task.mjs ${task.name}`,
                cwd: project.dir,
            },
        };
    }
    return `${JSON.stringify(
        {
            name: project.name,
            $schema: "../../node_modules/nx/schemas/project-schema.json",
            targets,
        },
        null,
        2,
    )}\n`;
}

export const nxAdapter: ToolAdapter = {
    tool: "nx",
    supportedVersions: [">=21.0.0 <24.0.0"],
    description:
        "Nx. Uses the Nx Daemon (a background process plus a results database) to accelerate repeated runs, toggled via the NX_DAEMON env var; installed as a workspace devDependency.",
    daemon: {
        hasDaemon: true,
        startMode: "auto",
        stopDaemon: async (ctx: ToolContext) => {
            await execa(resolveBin(ctx.rootDir, "nx"), ["reset"], {
                cwd: ctx.rootDir,
                reject: false,
                stdio: "ignore",
                env: { NX_DAEMON: "false" },
            });
        },
        daemonPids: async (ctx: ToolContext) => {
            if (!ctx.daemon) return [];
            // `nx daemon` prints the running daemon's background process ID and
            // log path (see https://nx.dev/concepts/nx-daemon#logs). Parse the PID.
            const result = await execa(
                resolveBin(ctx.rootDir, "nx"),
                ["daemon"],
                {
                    cwd: ctx.rootDir,
                    reject: false,
                    env: { NX_DAEMON: "true", NX_TUI: "false" },
                },
            ).catch(() => null);
            if (!result) return [];
            const text = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
            const match = text.match(/process id[^\d]*(\d+)/i);
            const pid = match ? Number(match[1]) : Number.NaN;
            return Number.isInteger(pid) && pid > 0 ? [pid] : [];
        },
    },

    pinnedVersion: (config) => config.versions.nx,
    devDependencies: (config) => ({ nx: config.versions.nx }),
    setup: async (ctx: GenerationContext) => {
        await ctx.write("nx.json", nxRootConfig(ctx.model));
        for (const project of ctx.projects) {
            await ctx.write(
                `${project.dir}/project.json`,
                nxProjectConfig(project),
            );
        }
    },

    run: (task, ctx) => ({
        file: resolveBin(ctx.rootDir, "nx"),
        args: ["run-many", "-t", task, `--parallel=${ctx.concurrency}`],
    }),
    env: (ctx) => ({
        NX_DAEMON: ctx.daemon ? "true" : "false",
        NX_TUI: "false",
    }),
    clearCaches: async (ctx: ToolContext) => {
        await removeDist(ctx);
        // nx 23 stores task results in a DB under `.nx/workspace-data` (used
        // even in no-daemon mode), which `--onlyCache` does NOT clear. A full
        // `nx reset` is the only reliable way to force a genuinely cold run.
        // clearCaches is only invoked for cold runs, so tearing down the daemon
        // too is acceptable (warm runs never call this).
        await execa(resolveBin(ctx.rootDir, "nx"), ["reset"], {
            cwd: ctx.rootDir,
            reject: false,
            stdio: "ignore",
            env: { NX_DAEMON: ctx.daemon ? "true" : "false", NX_TUI: "false" },
        });
    },
};
