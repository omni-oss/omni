/**
 * Create disposable temp workspaces for e2e tests.
 *
 * Every test should run against a fresh workspace so tests stay isolated and
 * never mutate repo fixtures in place. {@link makeWorkspace} builds a temp
 * directory containing a `workspace.omni.yaml` plus any project/config files,
 * and (when called inside a test) registers automatic cleanup.
 */

import {
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, isAbsolute, join } from "node:path";
import { stringify as stringifyToml } from "smol-toml";
import { onTestFinished } from "vitest";
import { stringify as stringifyYaml } from "yaml";
import { cleanPath } from "@/utils";

const SUPPORTED_EXT = /\.(ya?ml|json|toml)$/i;

/** A value that becomes file contents: a raw string, or an object → JSON. */
export type FileContent = string | Record<string, unknown> | unknown[];

export interface WorkspaceSpec {
    /**
     * Contents of `workspace.omni.yaml`. Objects are serialized to the file's
     * format (YAML by default; `.json`/`.toml` when the path says so).
     * Defaults to `{ projects: ["**"] }`.
     */
    workspace?: FileContent;
    /**
     * Projects to create. Each key is either:
     *   - a directory (e.g. `"apps/web"`) → writes `<dir>/project.omni.yaml`, or
     *   - an explicit config path (e.g. `"apps/web/project.omni.json"`).
     * Object values are serialized by extension (YAML default, else JSON/TOML).
     */
    projects?: Record<string, FileContent>;
    /** Arbitrary additional files keyed by workspace-relative path. */
    files?: Record<string, string>;
    /** Skip auto-registering cleanup with the current test. Default false. */
    noAutoCleanup?: boolean;
}

export interface Workspace {
    /** Absolute path to the workspace root. Use as `cwd` for {@link runOmni}. */
    readonly cwd: string;
    /** Alias of {@link Workspace.cwd}. */
    readonly root: string;
    /** Resolve an absolute path within the workspace. */
    path(...segments: string[]): string;
    /** Write (or overwrite) a file, creating parent directories. */
    write(relPath: string, content: FileContent): void;
    /** Read a workspace file as UTF-8. */
    read(relPath: string): string;
    /** True if the workspace-relative path exists. */
    exists(relPath: string): boolean;
    /** Recursively remove the workspace directory. */
    cleanup(): void;
}

function serialize(content: FileContent, relPath: string): string {
    if (typeof content === "string") {
        return content;
    }
    // Object specs are serialized to match the file extension; omni config
    // files default to YAML.
    switch (extname(relPath).toLowerCase()) {
        case ".json":
            return `${JSON.stringify(content, null, 2)}\n`;
        case ".toml":
            return stringifyToml(content as Record<string, unknown>);
        default:
            return stringifyYaml(content);
    }
}

function tryRegisterCleanup(cleanup: () => void): void {
    try {
        // `onTestFinished` throws when there is no active test context, which
        // lets the harness also be used from setup code that owns cleanup().
        onTestFinished(() => {
            try {
                cleanup();
            } catch {
                // Swallow cleanup errors so they don't mask the real test
                // failure.  On Windows, EPERM can occur when spawned child
                // processes still hold open file handles at teardown time
                // (e.g. after a test timeout).
            }
        });
    } catch {
        // Not inside a test - caller is responsible for calling cleanup().
    }
}

/**
 * Create a fresh temp workspace and return helpers for interacting with it.
 *
 * When invoked inside a Vitest test, the workspace is removed automatically
 * after the test finishes. Outside a test, call {@link Workspace.cleanup}.
 *
 * @example
 * const ws = makeWorkspace({
 *   projects: {
 *     "app": { name: "app", tasks: { build: 'echo "hi"' } },
 *   },
 * });
 * const result = await runOmni(["run", "build"], { cwd: ws.cwd });
 */
export function makeWorkspace(spec: WorkspaceSpec = {}): Workspace {
    const root = mkdtempSync(join(tmpdir(), "omni-e2e-"));

    const ws: Workspace = {
        cwd: root,
        root,
        path: (...segments) => {
            const result = join(root, ...segments);

            return cleanPath(result);
        },
        write(relPath, content) {
            const abs = isAbsolute(relPath) ? relPath : join(root, relPath);
            mkdirSync(dirname(abs), { recursive: true });
            writeFileSync(abs, serialize(content, relPath), "utf8");
        },
        read: (relPath) =>
            readFileSync(
                isAbsolute(relPath) ? relPath : join(root, relPath),
                "utf8",
            ),
        exists: (relPath) =>
            existsSync(isAbsolute(relPath) ? relPath : join(root, relPath)),
        cleanup() {
            rmSync(root, { recursive: true, force: true });
        },
    };

    ws.write("workspace.omni.yaml", spec.workspace ?? { projects: ["**"] });

    for (const [key, content] of Object.entries(spec.projects ?? {})) {
        const relPath = SUPPORTED_EXT.test(key)
            ? key
            : join(key, "project.omni.yaml");
        ws.write(relPath, content);
    }

    for (const [relPath, content] of Object.entries(spec.files ?? {})) {
        ws.write(relPath, content);
    }

    if (!spec.noAutoCleanup) {
        tryRegisterCleanup(() => ws.cleanup());
    }

    return ws;
}
