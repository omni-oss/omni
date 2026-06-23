/**
 * `{{ output_dir }}` template variable – workspace-relative path guarantee.
 *
 * Cements the fix in `crates/omni_generator/src/execute_actions.rs` where
 * `omni_utils::path::clean` is now applied to `workspace_dir` before it is
 * passed to `pathdiff::diff_paths`.
 *
 * Root cause (Windows):
 *   `Context::new` canonicalises the workspace root via `fs_canonicalize`,
 *   which on Windows returns a verbatim-prefixed path (`\\?\C:\…`).
 *   Meanwhile the output directory is built from `env_current_dir()`, which
 *   returns a plain path (`C:\…`). Because the two paths have different prefix
 *   forms `pathdiff::diff_paths` returned `None`, causing `execute_actions` to
 *   fall back to inserting the raw absolute path into the Tera context instead
 *   of the workspace-relative diff.
 *
 * After the fix `clean()` strips the verbatim prefix before `diff_paths` is
 * called, so both paths share the same form and the relative computation
 * succeeds on every platform.
 *
 * Observable consequence: a generator action that references `{{ output_dir }}`
 * in its content now always receives a workspace-relative path (e.g. `out`)
 * rather than an absolute one (e.g. `C:\Temp\…\out` or `/tmp/…/out`).
 *
 * Pinned to:
 *   crates/omni_generator/src/execute_actions.rs
 *   crates/omni_utils/src/path.rs
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Normalize OS path separators to `/` so assertions are portable. */
function normPath(p: string): string {
    return p.trim().replace(/\\/g, "/");
}

const GENERATOR_SOURCE = [{ source: "local", path: "generators/**" }];

/**
 * A generator that writes the value of `{{ output_dir }}` (the Tera context
 * variable set by `execute_actions`) verbatim into `path.txt`.
 *
 * Before the fix this rendered as an absolute path on Windows; after the fix
 * it is always a workspace-relative path.
 */
function outputDirCaptureSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: GENERATOR_SOURCE,
        },
        projects: {
            "generators/capture/generator.omni.yaml": {
                name: "capture",
                description: "writes {{ output_dir }} to a file",
                actions: [
                    {
                        type: "add-content",
                        output_path: "path.txt",
                        content: "{{ output_dir }}",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("+generator @cli (output_dir template variable)", {
    tags: ["generator"],
}, () => {
    it("{{ output_dir }} is workspace-relative for a top-level output dir", async () => {
        // Single path segment: separator differences don't apply, and the
        // expected value is unambiguously "out" on every platform.
        const ws = makeWorkspace(outputDirCaptureSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "capture",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Before the fix on Windows: absolute path like "C:\Temp\…\out"
        // After the fix (all platforms): workspace-relative "out"
        expect(normPath(ws.read("out/path.txt"))).toBe("out");
    });

    it("{{ output_dir }} is workspace-relative for a nested output dir", async () => {
        // Two-segment path exercises the separator-normalisation branch:
        // on Windows diff_paths returns "nested\\out", which we normalise to
        // "nested/out" for a portable assertion.
        const ws = makeWorkspace(outputDirCaptureSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "capture",
                "-o",
                "nested/out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Normalise backslashes so the assertion is identical on POSIX/Windows.
        expect(normPath(ws.read("nested/out/path.txt"))).toBe("nested/out");
    });

    it("{{ output_dir }} is workspace-relative for a sub-generator via run_generator", async () => {
        // The fix is applied inside execute_actions, which is called for every
        // generator—including those invoked transitively via run_generator.
        // Both parent and child must see the relative path.
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: GENERATOR_SOURCE,
            },
            projects: {
                "generators/child/generator.omni.yaml": {
                    name: "child",
                    description: "captures output_dir to child.txt",
                    actions: [
                        {
                            type: "add-content",
                            output_path: "child.txt",
                            content: "{{ output_dir }}",
                        },
                    ],
                },
                "generators/parent/generator.omni.yaml": {
                    name: "parent",
                    description: "captures output_dir, then invokes child",
                    actions: [
                        {
                            type: "add-content",
                            output_path: "parent.txt",
                            content: "{{ output_dir }}",
                        },
                        { type: "run-generator", generator: "child" },
                    ],
                },
            },
            files: { ".omni/sources/generator/.keep": "" },
        });

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Parent's own action must see the relative path.
        expect(normPath(ws.read("out/parent.txt"))).toBe("out");
        // The sub-generator must also see the relative path, not the absolute one.
        expect(normPath(ws.read("out/child.txt"))).toBe("out");
    });

    it("{{ output_dir }} does not start with a drive letter or leading slash", async () => {
        // Explicit guard against the regression: an absolute path would begin
        // with a drive letter + colon (Windows) or a leading slash (POSIX).
        const ws = makeWorkspace(outputDirCaptureSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "capture",
                "-o",
                "outputs",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        const content = ws.read("outputs/path.txt").trim();
        // Must not be an absolute path on Windows (e.g. "C:\…" or "\\?\…").
        expect(content).not.toMatch(/^[A-Za-z]:[/\\]/);
        // Must not be an absolute path on POSIX (starts with "/").
        expect(content).not.toMatch(/^\//);
        // Must be the plain relative name.
        expect(normPath(content)).toBe("outputs");
    });
});
