/**
 * Shared git-remote fixture + reachability probes for the network-dependent
 * e2e tests (`omni init --git`, git generator sources).
 *
 * These tests clone a real, public repo, so they can only run when GitHub is
 * reachable. Two transports are covered:
 *   - HTTPS: works anonymously everywhere, so it is the default and is the only
 *     form gen-014 can use (the config `uri` must parse as a `url::Url`).
 *   - SSH: clones via the system `ssh` (gix shells out to it), using the
 *     machine's existing keys. That only works where an authorized key is
 *     present (typically dev machines, not CI), so SSH tests are gated on a
 *     separate probe and skip cleanly elsewhere.
 *
 * Probes are memoized so each transport is touched at most once per test file;
 * specs call {@link skipUnlessRemoteReachable} / {@link skipUnlessSshReachable}
 * to skip cleanly (rather than fail) when the remote is out of reach.
 */

import { execa } from "execa";

/** A test context that supports Vitest's conditional `skip(condition, note)`. */
type SkippableContext = {
    skip(condition: boolean, note?: string): void;
};

/**
 * Public, intentionally tiny omni workspace-template repo. Its root
 * `generator.omni.yaml` is a primary generator (one config at the source root),
 * which is exactly what `omni init` and git generator sources consume.
 */
export const workspaceMinimalRepo = {
    /** HTTPS clone URL - works anonymously, so it is CI-friendly. */
    https: "https://github.com/omni-oss/workspace-minimal.git",
    /**
     * SCP-style SSH remote. Valid for `init --git` (a raw string handed to gix)
     * but NOT for a generator-source `uri`, which must parse as a URL.
     */
    ssh: "git@github.com:omni-oss/workspace-minimal.git",
    /**
     * `ssh://` URL form of the SSH remote. This is a valid `url::Url`, so it is
     * the form usable in a git generator-source `uri`.
     */
    sshUrl: "ssh://git@github.com/omni-oss/workspace-minimal.git",
    /** Default branch pulled by the tests. */
    rev: "main",
    pinCommit: "cef04c0278eee8787171fb5f0dfee646f1d460ab",
    /** Identity of the repo's primary generator (see `generator.omni.yaml`). */
    generatorId: "workspace-minimal",
    generatorDisplayName: "Minimal Omni Workspace",
    generatorDescription: "Generates a minimal workspace",
    /** Name of the single text prompt the generator asks for. */
    promptName: "workspace_name",
} as const;

const PROBE_TIMEOUT_MS = 10_000;

// BatchMode makes ssh fail fast instead of prompting (so the probe can never
// hang on a passphrase / password); accept-new avoids a host-key prompt on a
// first connection.
const SSH_PROBE_COMMAND =
    "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10";

let httpsProbe: Promise<boolean> | undefined;
let sshProbe: Promise<boolean> | undefined;

/**
 * Whether {@link workspaceMinimalRepo} is reachable over HTTPS. Memoized: the
 * smart-HTTP advertised-refs endpoint is hit at most once per process.
 */
export function remoteReachable(): Promise<boolean> {
    httpsProbe ??= probeSmartHttp(
        `${workspaceMinimalRepo.https}/info/refs?service=git-upload-pack`,
    );
    return httpsProbe;
}

/**
 * Whether the SSH remote can be authenticated with the machine's existing keys
 * (via the system `ssh`, which is also what gix uses). Memoized; false when
 * `git`/`ssh` is missing or no authorized key is available.
 */
export function sshReachable(): Promise<boolean> {
    sshProbe ??= probeSsh(workspaceMinimalRepo.ssh);
    return sshProbe;
}

/**
 * Skip the current test when the remote can't be reached, so offline runs stay
 * green instead of failing on a network error.
 */
export async function skipUnlessRemoteReachable(
    ctx: SkippableContext,
): Promise<void> {
    ctx.skip(
        !(await remoteReachable()),
        "github.com/omni-oss/workspace-minimal is unreachable",
    );
}

/**
 * Skip the current test unless the SSH remote is reachable with the machine's
 * keys (e.g. on a dev box with an authorized key + ssh-agent). Keeps SSH-form
 * tests green on CI and contributors that lack SSH access.
 */
export async function skipUnlessSshReachable(
    ctx: SkippableContext,
): Promise<void> {
    ctx.skip(
        !(await sshReachable()),
        "SSH access to github.com/omni-oss/workspace-minimal is unavailable",
    );
}

async function probeSmartHttp(url: string): Promise<boolean> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
    try {
        const res = await fetch(url, {
            signal: controller.signal,
            redirect: "follow",
        });
        // Release the connection without buffering the body.
        await res.body?.cancel().catch(() => undefined);
        return res.ok;
    } catch {
        return false;
    } finally {
        clearTimeout(timer);
    }
}

async function probeSsh(scpUrl: string): Promise<boolean> {
    try {
        const res = await execa(
            "git",
            ["ls-remote", "--exit-code", scpUrl, "HEAD"],
            {
                reject: false,
                timeout: PROBE_TIMEOUT_MS,
                env: { GIT_SSH_COMMAND: SSH_PROBE_COMMAND },
            },
        );
        return res.exitCode === 0;
    } catch {
        // `git`/`ssh` missing, spawn failure, or timeout.
        return false;
    }
}
