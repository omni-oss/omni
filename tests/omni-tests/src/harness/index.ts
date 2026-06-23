/**
 * Public surface of the omni e2e test harness.
 *
 * Import everything you need from here:
 *
 * ```ts
 * import { makeWorkspace, runOmni } from "@/harness";
 * ```
 *
 * Custom matchers (`toSucceed`, `toOutputContaining`, ...) are registered
 * automatically by the harness setup file configured in `vitest.config.e2e.ts`.
 */

export type { CargoProfile, ResolveOmniBinOptions } from "./binary";
export { ensureOmniBinary, resolveOmniBin } from "./binary";
export {
    dependencyChainSpec,
    extendsSpec,
    multiFormatProjectsSpec,
    mutualRecursionGeneratorSpec,
    nestedGeneratorSpec,
    promptGeneratorSpec,
    scaffoldGeneratorSpec,
    selfRecursiveGeneratorSpec,
    singleProjectSpec,
} from "./fixtures";
export {
    remoteReachable,
    skipUnlessRemoteReachable,
    skipUnlessSshReachable,
    sshReachable,
    workspaceMinimalRepo,
} from "./git-remote";
export type { ConnectMcpOptions, McpClient } from "./mcp";
export { connectMcp } from "./mcp";
export { lines, normalize, normalizeLines } from "./normalize";
export { findRepoRoot } from "./paths";
export type {
    PtyExit,
    PtyKey,
    PtySession,
    ScreenMatcher,
    SpawnOmniPtyOptions,
    WaitForOptions,
} from "./pty";
export { spawnOmniPty } from "./pty";
export type { OmniResult, RunOmniOptions } from "./run-omni";
export { runOmni } from "./run-omni";
export type { FileContent, Workspace, WorkspaceSpec } from "./workspace";
export { makeWorkspace } from "./workspace";
