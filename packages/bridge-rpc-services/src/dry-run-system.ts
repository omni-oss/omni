import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { Log } from "@omni-oss/log";
import type {
    FileStat,
    FileSystem,
    Process,
    ProcessEnv,
    System,
} from "@omni-oss/system-interface";

export class DryRunSystem implements System {
    private constructor(
        public readonly fs: DryRunFileSystem,
        public readonly proc: DryRunProcess,
    ) {}

    public static async create(client: ClientHandle): Promise<DryRunSystem> {
        return new DryRunSystem(
            new DryRunFileSystem(client),
            new DryRunProcess(client, "", [], {}),
        );
    }
}

export class DryRunFileSystem implements FileSystem {
    constructor(_client: ClientHandle) {
        Log.warn(
            "DryRunFileSystem instantiated. All file system operations will throw an error.",
        );
    }

    readFileAsString(_path: string): Promise<string> {
        throw new Error("Method not implemented.");
    }
    readFileAsBytes(_path: string): Promise<Uint8Array> {
        throw new Error("Method not implemented.");
    }
    writeStringToFile(_path: string, _content: string): Promise<void> {
        throw new Error("Method not implemented.");
    }
    writeBytesToFile(_path: string, _content: Uint8Array): Promise<void> {
        throw new Error("Method not implemented.");
    }
    pathExists(_path: string): Promise<boolean> {
        throw new Error("Method not implemented.");
    }
    createDirectory(
        _path: string,
        _options?: { recursive?: boolean },
    ): Promise<void> {
        throw new Error("Method not implemented.");
    }
    readDirectory(_path: string): Promise<string[]> {
        throw new Error("Method not implemented.");
    }
    remove(_path: string, _options?: { recursive?: boolean }): Promise<void> {
        throw new Error("Method not implemented.");
    }
    rename(_oldPath: string, _newPath: string): Promise<void> {
        throw new Error("Method not implemented.");
    }
    stat(_path: string): Promise<FileStat> {
        throw new Error("Method not implemented.");
    }
    isFile(_path: string): Promise<boolean> {
        throw new Error("Method not implemented.");
    }
    isDirectory(_path: string): Promise<boolean> {
        throw new Error("Method not implemented.");
    }
    isSymbolicLink(_path: string): Promise<boolean> {
        throw new Error("Method not implemented.");
    }
    copy(
        _src: string,
        _dest: string,
        _options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        throw new Error("Method not implemented.");
    }
    appendStringToFile(_path: string, _content: string): Promise<void> {
        throw new Error("Method not implemented.");
    }
}

export class DryRunProcess implements Process {
    constructor(
        _client: ClientHandle,
        private currentDirSnapshot: string,
        private argsList: string[],
        private envVars: ProcessEnv,
    ) {
        Log.warn(
            "DryRunProcess instantiated. All process operations will throw an error.",
        );
    }

    currentDir(): string {
        return this.currentDirSnapshot;
    }
    setCurrentDir(dir: string): Promise<void> {
        this.currentDirSnapshot = dir;
        throw new Error("Method not implemented.");
    }
    args(): string[] {
        return this.argsList;
    }
    env(): ProcessEnv {
        return this.envVars;
    }
}
