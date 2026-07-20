/**
 * Configuration for the Bridge RPC routes that
 * {@link BridgeRpcSystem} talks to.
 *
 * The defaults match the Rust services exposed by the `bridge_rpc_services`
 * crate (see `crates/bridge_rpc_services/src/services/register.rs`). All
 * fields are optional - any subset can be overridden via
 * {@link createRpcSystemOptions}.
 */
import type { EnvRuleLayers } from "./env-capability";

export type BridgeRpcSystemOptions = {
    /**
     * Path prefix prepended to every file-system route. Defaults to
     * {@link DEFAULT_FS_PREFIX}.
     */
    fsPrefix: string;
    /**
     * Path prefix prepended to every process route. Defaults to
     * {@link DEFAULT_PROC_PREFIX}.
     */
    procPrefix: string;
    /**
     * Maximum size, in bytes, of a single body chunk emitted on a request.
     * Bodies larger than this are split into multiple body-chunk frames.
     * Defaults to {@link DEFAULT_MAX_CHUNK_SIZE}.
     */
    maxChunkSize: number;
    /**
     * Layered `env` capability rules to enforce on `proc.env()` reads, the same
     * rules handed to the runtime shim. When present (even an empty array,
     * which denies everything), `proc.env()` returns a capability-filtered view
     * so a script only observes the variables its generator is permitted to
     * read. When omitted, the RPC snapshot is exposed verbatim (the Rust broker
     * has already filtered it).
     */
    envRules?: EnvRuleLayers | undefined;
};

export type PartialBridgeRpcSystemOptions = Partial<BridgeRpcSystemOptions>;

/** Default file-system prefix matching the Rust default. */
export const DEFAULT_FS_PREFIX = "/fs";
/** Default process prefix matching the Rust default. */
export const DEFAULT_PROC_PREFIX = "/proc";
/** Default body chunk size: 64 KiB. */
export const DEFAULT_MAX_CHUNK_SIZE = 64 * 1024;

/** Header key under which **request** parameters are transported. */
export const PARAMETERS_HEADER = "parameters";

/** Header key under which **response** return values are transported. */
export const RETURNS_HEADER = "returns";

/**
 * File-system route names (relative to the FS prefix). These mirror
 * `fs_routes` in the Rust crate.
 */
export const FS_ROUTES = {
    READ_FILE_AS_STRING: "/read-file-as-string",
    READ_FILE_AS_BYTES: "/read-file-as-bytes",
    WRITE_STRING_TO_FILE: "/write-string-to-file",
    WRITE_BYTES_TO_FILE: "/write-bytes-to-file",
    PATH_EXISTS: "/path-exists",
    CREATE_DIRECTORY: "/create-directory",
    READ_DIRECTORY: "/read-directory",
    REMOVE: "/remove",
    RENAME: "/rename",
    STAT: "/stat",
    IS_FILE: "/is-file",
    IS_DIRECTORY: "/is-directory",
    IS_SYMBOLIC_LINK: "/is-symbolic-link",
    COPY: "/copy",
    APPEND_STRING_TO_FILE: "/append-string-to-file",
} as const;

/**
 * Process route names (relative to the proc prefix). Mirrors `proc_routes`
 * in the Rust crate.
 */
export const PROC_ROUTES = {
    CURRENT_DIR: "/current-dir",
    SET_CURRENT_DIR: "/set-current-dir",
    ARGS: "/args",
    ENV: "/env",
    SNAPSHOT: "/snapshot",
} as const;

/**
 * Resolves a partial options object to a fully-populated one, filling in
 * defaults for any missing fields.
 */
export function createRpcSystemOptions(
    overrides?: PartialBridgeRpcSystemOptions,
): BridgeRpcSystemOptions {
    return {
        fsPrefix: overrides?.fsPrefix ?? DEFAULT_FS_PREFIX,
        procPrefix: overrides?.procPrefix ?? DEFAULT_PROC_PREFIX,
        maxChunkSize: overrides?.maxChunkSize ?? DEFAULT_MAX_CHUNK_SIZE,
        envRules: overrides?.envRules,
    };
}

/**
 * Joins a prefix and a route, normalising trailing/leading slashes.
 */
export function joinRoute(prefix: string, route: string): string {
    const trimmedPrefix = prefix.replace(/\/+$/, "");
    const trimmedRoute = route.replace(/^\/+/, "");
    return `${trimmedPrefix}/${trimmedRoute}`;
}
