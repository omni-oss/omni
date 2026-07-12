import { satisfies } from "semver";
import type { HarnessConfig, Tool } from "../config";
import { moonAdapter } from "./moon";
import { nxAdapter } from "./nx";
import { omniAdapter } from "./omni";
import { turboAdapter } from "./turbo";
import type { ToolAdapter, ToolInfo } from "./types";

const ADAPTERS: Record<Tool, ToolAdapter> = {
    omni: omniAdapter,
    turbo: turboAdapter,
    nx: nxAdapter,
    moon: moonAdapter,
};

export function getAdapter(tool: Tool): ToolAdapter {
    return ADAPTERS[tool];
}

export function getAdapters(tools: Tool[]): ToolAdapter[] {
    return tools.map(getAdapter);
}

/**
 * Summarize a tool's noteworthy attributes (version plus daemon/provisioning/
 * supported ranges and a description). `provisioning` is derived from whether
 * the adapter contributes any workspace devDependencies.
 */
export function describeTool(
    tool: Tool,
    config: HarnessConfig,
    version: string | null,
): ToolInfo {
    const adapter = getAdapter(tool);
    const installsDeps =
        Object.keys(adapter.devDependencies(config)).length > 0;
    return {
        tool,
        version,
        daemon: adapter.daemon?.hasDaemon ?? false,
        provisioning: installsDeps ? "workspace-dependency" : "host-binary",
        supportedVersions: [...adapter.supportedVersions],
        description: adapter.description,
    };
}

/** Throw if `version` does not satisfy any of the adapter's supported ranges. */
export function assertSupportedVersion(
    adapter: ToolAdapter,
    version: string,
): void {
    const ok = adapter.supportedVersions.some((range) =>
        satisfies(version, range, { includePrerelease: true, loose: true }),
    );
    if (!ok) {
        throw new Error(
            `${adapter.tool} version "${version}" is not supported by task-bench ` +
                `(supported: ${adapter.supportedVersions.join(" || ")}). ` +
                `Adjust versions.${adapter.tool} in your config or upgrade/downgrade the tool.`,
        );
    }
}

/**
 * Resolve each enabled tool's version (pinned via config, or detected for
 * external tools) and validate it against the adapter's supported ranges.
 * Throws on the first unsupported version. Returns a map of tool -> version.
 */
export async function resolveToolVersions(
    config: HarnessConfig,
    rootDir: string,
    tools: Tool[] = config.tools,
): Promise<Map<Tool, string | null>> {
    const versions = new Map<Tool, string | null>();
    for (const tool of tools) {
        const adapter = getAdapter(tool);
        let version = adapter.pinnedVersion(config);
        if (version === null && adapter.detectVersion) {
            version = await adapter.detectVersion(rootDir);
        }
        if (version !== null) {
            assertSupportedVersion(adapter, version);
        }
        versions.set(tool, version);
    }
    return versions;
}

export {
    moonAdapter,
    moonProjectConfig,
    moonToolchainConfig,
    moonWorkspaceConfig,
} from "./moon";
export { nxAdapter, nxProjectConfig, nxRootConfig } from "./nx";
export { OMNI_RENDER_OPTIONS, omniAdapter } from "./omni";
export { turboAdapter, turboRootConfig } from "./turbo";
export * from "./types";
