/**
 * Build disposable *local* git repositories for e2e tests that need to clone
 * without touching the network.
 *
 * Unlike {@link makeWorkspace}, this initializes a real git repo (commit +
 * annotated tag) that `omni init --git <path>` / `clone_repo` can clone over
 * the local transport. It exists mainly to cover revision-resolution behavior
 * (e.g. cloning at an *annotated* tag, whose object OID differs from the commit
 * it points at) hermetically and deterministically.
 *
 * `git` is invoked with an isolated config (no user/system config, fixed
 * identity, no prompts) so results don't depend on the host's git setup.
 */

import {
    mkdirSync,
    mkdtempSync,
    realpathSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { execa } from "execa";
import { onTestFinished } from "vitest";
import { cleanPath } from "@/utils";

/** A test context that supports Vitest's conditional `skip(condition, note)`. */
type SkippableContext = {
    skip(condition: boolean, note?: string): void;
};

const PROBE_TIMEOUT_MS = 10_000;

// Isolate git from the host: no user/system config (which could enforce GPG
// signing, hooks, or a template dir), a fixed author/committer identity, and no
// credential/terminal prompts. A missing GIT_CONFIG_GLOBAL path is treated as
// an empty config on every platform.
const ISOLATED_GIT_ENV = {
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: "/dev/null",
    GIT_TERMINAL_PROMPT: "0",
    GIT_AUTHOR_NAME: "omni e2e",
    GIT_AUTHOR_EMAIL: "omni-e2e@example.com",
    GIT_COMMITTER_NAME: "omni e2e",
    GIT_COMMITTER_EMAIL: "omni-e2e@example.com",
} as const;

let gitProbe: Promise<boolean> | undefined;

/** Whether a usable `git` binary is on PATH. Memoized per process. */
export function gitCliAvailable(): Promise<boolean> {
    gitProbe ??= execa("git", ["--version"], {
        reject: false,
        timeout: PROBE_TIMEOUT_MS,
    })
        .then((r) => r.exitCode === 0)
        .catch(() => false);
    return gitProbe;
}

/**
 * Skip the current test when no `git` binary is available, so environments
 * without git stay green instead of failing on a spawn error.
 */
export async function skipUnlessGitCliAvailable(
    ctx: SkippableContext,
): Promise<void> {
    ctx.skip(!(await gitCliAvailable()), "`git` is not available on PATH");
}

export interface LocalGitRepoSpec {
    /** Files committed and then annotated-tagged, keyed by repo-relative path. */
    files: Record<string, string>;
    /** Annotated tag name to create at the initial commit. Default `v1.0.0`. */
    tag?: string;
    /**
     * Optional files committed *after* the tag (advancing the branch tip past
     * it). Use this to prove a clone resolved the tag, not the branch head.
     */
    postTagFiles?: Record<string, string>;
}

export interface LocalGitRepo {
    /** Filesystem path to hand to `--git` / `clone_repo` (local transport). */
    readonly url: string;
    /** The annotated tag name created at the initial commit. */
    readonly tag: string;
    /** Commit OID the annotated tag points at. */
    readonly taggedCommit: string;
    /** Branch tip OID (differs from {@link taggedCommit} when `postTagFiles` is set). */
    readonly headCommit: string;
    /** Annotated tag *object* OID (always differs from {@link taggedCommit}). */
    readonly tagObject: string;
    /** Remove the repo directory. */
    cleanup(): void;
}

/**
 * Create a local git repo containing `spec.files`, committed and marked with an
 * annotated tag. When invoked inside a test, the repo is auto-removed on finish.
 *
 * Requires a `git` binary; gate the test with {@link skipUnlessGitCliAvailable}.
 */
export async function makeLocalGitRepoWithAnnotatedTag(
    spec: LocalGitRepoSpec,
): Promise<LocalGitRepo> {
    const tag = spec.tag ?? "v1.0.0";
    const repoRoot = cleanPath(
        realpathSync.native(mkdtempSync(join(tmpdir(), "omni-e2e-gitrepo-"))),
    );

    const git = (args: string[]) =>
        execa("git", args, { cwd: repoRoot, env: ISOLATED_GIT_ENV });

    const write = (files: Record<string, string>) => {
        for (const [rel, content] of Object.entries(files)) {
            const abs = join(repoRoot, rel);
            mkdirSync(dirname(abs), { recursive: true });
            writeFileSync(abs, content, "utf8");
        }
    };

    await git(["init", "-q", "-b", "main"]);

    write(spec.files);
    await git(["add", "-A"]);
    await git(["commit", "-q", "-m", "initial commit"]);
    await git(["tag", "-a", tag, "-m", `release ${tag}`]);

    const taggedCommit = (await git(["rev-parse", "HEAD"])).stdout.trim();
    const tagObject = (await git(["rev-parse", tag])).stdout.trim();

    if (spec.postTagFiles) {
        write(spec.postTagFiles);
        await git(["add", "-A"]);
        await git(["commit", "-q", "-m", "post-tag commit"]);
    }

    const headCommit = (await git(["rev-parse", "HEAD"])).stdout.trim();

    const repo: LocalGitRepo = {
        url: repoRoot,
        tag,
        taggedCommit,
        headCommit,
        tagObject,
        cleanup() {
            rmSync(repoRoot, { recursive: true, force: true });
        },
    };

    tryRegisterCleanup(() => repo.cleanup());

    return repo;
}

function tryRegisterCleanup(cleanup: () => void): void {
    try {
        onTestFinished(() => {
            try {
                cleanup();
            } catch {
                // Swallow cleanup errors so they don't mask the real failure.
            }
        });
    } catch {
        // Not inside a test - caller owns cleanup().
    }
}
