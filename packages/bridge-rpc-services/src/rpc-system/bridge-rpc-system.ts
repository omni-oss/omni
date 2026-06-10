import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import type {
    ArgsList,
    FileStat,
    FileSystem,
    Process,
    ProcessEnv,
    System,
} from "@omni-oss/system-interface";
import {
    type BridgeRpcSystemOptions,
    createRpcSystemOptions,
    FS_ROUTES,
    joinRoute,
    type PartialBridgeRpcSystemOptions,
    PROC_ROUTES,
} from "./options";
import {
    asNumber,
    callExpectingBody,
    callWithBody,
    callWithParameters,
    readResponseBody,
    readResponseReturns,
} from "./rpc";

const TEXT_DECODER = new TextDecoder("utf-8", { fatal: true });
const TEXT_ENCODER = new TextEncoder();

/* ------------------------------------------------------------------------- */
/* Wire payload shapes                                                        */
/* ------------------------------------------------------------------------- */

type BoolResponse = { value: boolean };
type ReadDirectoryResponse = { entries: string[] };
type StatResponse = {
    is_file: boolean;
    is_directory: boolean;
    is_symbolic_link: boolean;
    size: number | bigint;
    /** Last-modified time, in milliseconds since the Unix epoch. */
    mtime_ms: number | bigint;
};
type ProcessSnapshotResponse = {
    current_dir: string;
    args: string[];
    env: Record<string, string>;
};

/* ------------------------------------------------------------------------- */
/* BridgeRpcSystem                                                            */
/* ------------------------------------------------------------------------- */

/**
 * `System` implementation backed by Bridge RPC services running on the
 * other end of a {@link ClientHandle} (typically the Rust side). All
 * file-system and process operations are delegated over RPC, using the
 * conventions defined in
 * `crates/bridge_rpc_services/src/services/common.rs`:
 *
 * - "Trivial" parameters are passed in the `parameters` header.
 * - Bulk content (file bodies) is passed in the body, split into chunks of
 *   at most {@link BridgeRpcSystemOptions.maxChunkSize} bytes.
 *
 * Default route prefixes match the Rust defaults (`/fs` and `/proc`) but
 * can be overridden via {@link create}.
 */
export class BridgeRpcSystem implements System {
    private constructor(
        public readonly fs: BridgeRpcFileSystem,
        public readonly proc: BridgeRpcProcess,
    ) {}

    /**
     * Builds a fully-initialised `BridgeRpcSystem`. Performs a single RPC
     * call to `<procPrefix>/snapshot` to populate the synchronous accessors
     * on {@link BridgeRpcProcess}.
     */
    public static async create(
        client: ClientHandle,
        options?: PartialBridgeRpcSystemOptions,
    ): Promise<BridgeRpcSystem> {
        const resolved = createRpcSystemOptions(options);

        const fs = new BridgeRpcFileSystem(client, resolved);
        const proc = await BridgeRpcProcess.create(client, resolved);

        return new BridgeRpcSystem(fs, proc);
    }
}

/* ------------------------------------------------------------------------- */
/* BridgeRpcFileSystem                                                        */
/* ------------------------------------------------------------------------- */

export class BridgeRpcFileSystem implements FileSystem {
    constructor(
        private readonly client: ClientHandle,
        private readonly options: BridgeRpcSystemOptions,
    ) {}

    private path(route: string): string {
        return joinRoute(this.options.fsPrefix, route);
    }

    async readFileAsString(path: string): Promise<string> {
        const route = this.path(FS_ROUTES.READ_FILE_AS_STRING);
        const response = await callExpectingBody(this.client, route, {
            path,
        });
        const body = await readResponseBody(response);
        return TEXT_DECODER.decode(body);
    }

    async readFileAsBytes(path: string): Promise<Uint8Array> {
        const route = this.path(FS_ROUTES.READ_FILE_AS_BYTES);
        const response = await callExpectingBody(this.client, route, {
            path,
        });
        return await readResponseBody(response);
    }

    async writeStringToFile(path: string, content: string): Promise<void> {
        const route = this.path(FS_ROUTES.WRITE_STRING_TO_FILE);
        const body = TEXT_ENCODER.encode(content);
        await callWithBody(
            this.client,
            route,
            { path },
            body,
            this.options.maxChunkSize,
        );
    }

    async writeBytesToFile(path: string, content: Uint8Array): Promise<void> {
        const route = this.path(FS_ROUTES.WRITE_BYTES_TO_FILE);
        await callWithBody(
            this.client,
            route,
            { path },
            content,
            this.options.maxChunkSize,
        );
    }

    async pathExists(path: string): Promise<boolean> {
        const route = this.path(FS_ROUTES.PATH_EXISTS);
        const response = await callWithParameters(this.client, route, {
            path,
        });
        return readResponseReturns<BoolResponse>(response).value;
    }

    async createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        const route = this.path(FS_ROUTES.CREATE_DIRECTORY);
        await callWithParameters(this.client, route, {
            path,
            options: { recursive: options?.recursive ?? false },
        });
    }

    async readDirectory(path: string): Promise<string[]> {
        const route = this.path(FS_ROUTES.READ_DIRECTORY);
        const response = await callWithParameters(this.client, route, {
            path,
        });
        return readResponseReturns<ReadDirectoryResponse>(response).entries;
    }

    async remove(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        const route = this.path(FS_ROUTES.REMOVE);
        await callWithParameters(this.client, route, {
            path,
            options: { recursive: options?.recursive ?? false },
        });
    }

    async rename(oldPath: string, newPath: string): Promise<void> {
        const route = this.path(FS_ROUTES.RENAME);
        await callWithParameters(this.client, route, {
            old_path: oldPath,
            new_path: newPath,
        });
    }

    async stat(path: string): Promise<FileStat> {
        const route = this.path(FS_ROUTES.STAT);
        const response = await callWithParameters(this.client, route, {
            path,
        });
        const raw = readResponseReturns<StatResponse>(response);
        return new BridgeRpcFileStat(raw);
    }

    async isFile(path: string): Promise<boolean> {
        const route = this.path(FS_ROUTES.IS_FILE);
        const response = await callWithParameters(this.client, route, {
            path,
        });
        return readResponseReturns<BoolResponse>(response).value;
    }

    async isDirectory(path: string): Promise<boolean> {
        const route = this.path(FS_ROUTES.IS_DIRECTORY);
        const response = await callWithParameters(this.client, route, {
            path,
        });
        return readResponseReturns<BoolResponse>(response).value;
    }

    async isSymbolicLink(path: string): Promise<boolean> {
        const route = this.path(FS_ROUTES.IS_SYMBOLIC_LINK);
        const response = await callWithParameters(this.client, route, {
            path,
        });
        return readResponseReturns<BoolResponse>(response).value;
    }

    async copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        const route = this.path(FS_ROUTES.COPY);
        await callWithParameters(this.client, route, {
            src,
            dest,
            options: {
                overwrite: options?.overwrite ?? false,
                recursive: options?.recursive ?? false,
            },
        });
    }

    async appendStringToFile(path: string, content: string): Promise<void> {
        const route = this.path(FS_ROUTES.APPEND_STRING_TO_FILE);
        const body = TEXT_ENCODER.encode(content);
        await callWithBody(
            this.client,
            route,
            { path },
            body,
            this.options.maxChunkSize,
        );
    }
}

/**
 * Concrete `FileStat` returned by {@link BridgeRpcFileSystem.stat}. The
 * underlying wire payload uses `snake_case` field names (matching the Rust
 * services); this wrapper adapts that to the JS-friendly `FileStat`
 * interface.
 */
class BridgeRpcFileStat implements FileStat {
    public readonly size: number;
    public readonly mtime: Date;

    constructor(private readonly raw: StatResponse) {
        this.size = asNumber(raw.size);
        this.mtime = new Date(asNumber(raw.mtime_ms));
    }

    isFile(): boolean {
        return this.raw.is_file;
    }
    isDirectory(): boolean {
        return this.raw.is_directory;
    }
    isSymbolicLink(): boolean {
        return this.raw.is_symbolic_link;
    }
}

/* ------------------------------------------------------------------------- */
/* BridgeRpcProcess                                                           */
/* ------------------------------------------------------------------------- */

export class BridgeRpcProcess implements Process {
    private constructor(
        private readonly client: ClientHandle,
        private readonly options: BridgeRpcSystemOptions,
        private currentDirSnapshot: string,
        private readonly argsList: string[],
        private readonly envVars: {
            [key: string]: string | undefined;
        },
    ) {}

    /**
     * Loads the initial process snapshot from `<procPrefix>/snapshot` so
     * that the synchronous accessors (`currentDir`, `args`, `env`) can
     * return cached values that are consistent with the host process at
     * construction time.
     */
    public static async create(
        client: ClientHandle,
        options: BridgeRpcSystemOptions,
    ): Promise<BridgeRpcProcess> {
        const route = joinRoute(options.procPrefix, PROC_ROUTES.SNAPSHOT);
        const response = await callWithParameters(client, route);
        const snapshot = readResponseReturns<ProcessSnapshotResponse>(response);

        return new BridgeRpcProcess(
            client,
            options,
            snapshot.current_dir,
            snapshot.args,
            snapshot.env,
        );
    }

    private path(route: string): string {
        return joinRoute(this.options.procPrefix, route);
    }

    currentDir(): string {
        return this.currentDirSnapshot;
    }

    async setCurrentDir(dir: string): Promise<void> {
        const route = this.path(PROC_ROUTES.SET_CURRENT_DIR);
        await callWithParameters(this.client, route, { dir });
        // Only update the cached value if the RPC succeeded.
        this.currentDirSnapshot = dir;
    }

    args(): ArgsList {
        return this.argsList as readonly string[];
    }

    env(): ProcessEnv {
        return this.envVars;
    }

    /**
     * Forces a refresh of the cached snapshot from the host process.
     *
     * Useful when the host process's environment may have changed since
     * `create` was called and the JS side wants to observe the new state.
     * This is in addition to the standard `Process` interface.
     */
    public async refreshSnapshot(): Promise<void> {
        const route = this.path(PROC_ROUTES.SNAPSHOT);
        const response = await callWithParameters(this.client, route);
        const snapshot = readResponseReturns<ProcessSnapshotResponse>(response);

        this.currentDirSnapshot = snapshot.current_dir;
        // Mutate in place so existing references (returned by `args()` /
        // `env()`) stay consistent.
        this.argsList.length = 0;
        this.argsList.push(...snapshot.args);
        for (const key of Object.keys(this.envVars)) {
            delete this.envVars[key];
        }
        for (const [key, value] of Object.entries(snapshot.env)) {
            this.envVars[key] = value;
        }
    }
}
