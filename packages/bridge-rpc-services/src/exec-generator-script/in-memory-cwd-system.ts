import nodePath from "node:path";
import type {
    ArgsList,
    Env,
    FileStat,
    FileSystem,
    Process,
    System,
} from "@omni-oss/system-interface";

/* ------------------------------------------------------------------------- */
/* Shared in-memory current-directory state                                  */
/* ------------------------------------------------------------------------- */

/**
 * Holds the in-memory current working directory shared between the
 * {@link InMemoryCwdProcess} (which mutates it) and the
 * {@link InMemoryCwdFileSystem} (which reads it to resolve relative paths).
 *
 * The directory is always kept absolute so that subsequent relative
 * resolutions are stable and never depend on the host process's real working
 * directory.
 */
class InMemoryCwdState {
    private dir: string;

    constructor(initialDir: string) {
        // Normalise to an absolute path up-front so that relative resolutions
        // are deterministic regardless of the host process state.
        this.dir = nodePath.resolve(initialDir);
    }

    /** The current in-memory working directory (always absolute). */
    get current(): string {
        return this.dir;
    }

    /**
     * Updates the in-memory working directory. Relative `dir` values are
     * resolved against the existing current directory; absolute values
     * replace it outright.
     */
    set(dir: string): void {
        this.dir = nodePath.resolve(this.dir, dir);
    }

    /**
     * Resolves `path` against the in-memory current directory. Absolute paths
     * are returned unchanged (after normalisation); relative paths are made
     * absolute relative to {@link current}.
     */
    resolve(path: string): string {
        return nodePath.resolve(this.dir, path);
    }
}

/* ------------------------------------------------------------------------- */
/* InMemoryCwdSystem                                                          */
/* ------------------------------------------------------------------------- */

/**
 * `System` wrapper that virtualises the current working directory.
 *
 * The wrapped system's `proc.currentDir()` / `proc.setCurrentDir()` are
 * intercepted so that current-directory changes are tracked purely in memory
 * and never propagated to the underlying (host) process. Every file-system
 * operation that takes a path resolves relative paths against this in-memory
 * directory before delegating to the wrapped system, so scripts can `cd`
 * around and use relative paths without ever mutating the real host process.
 *
 * The in-memory directory starts out at the `initialDir` passed to
 * {@link wrap} (typically the generator's `outputDir`).
 */
export class InMemoryCwdSystem implements System {
    private constructor(
        public readonly fs: InMemoryCwdFileSystem,
        public readonly proc: InMemoryCwdProcess,
    ) {}

    /**
     * Wraps `system`, seeding the in-memory current directory with
     * `initialDir`.
     *
     * @param system The system whose file-system operations are delegated to.
     * @param initialDir The initial in-memory current working directory.
     */
    public static wrap(system: System, initialDir: string): InMemoryCwdSystem {
        const state = new InMemoryCwdState(initialDir);
        const fs = new InMemoryCwdFileSystem(system.fs, state);
        const proc = new InMemoryCwdProcess(system.proc, state);
        return new InMemoryCwdSystem(fs, proc);
    }
}

/* ------------------------------------------------------------------------- */
/* InMemoryCwdFileSystem                                                      */
/* ------------------------------------------------------------------------- */

/**
 * `FileSystem` wrapper that resolves every incoming path against an in-memory
 * current working directory before delegating to the inner file system.
 */
export class InMemoryCwdFileSystem implements FileSystem {
    constructor(
        private readonly inner: FileSystem,
        private readonly state: InMemoryCwdState,
    ) {}

    readFileAsString(path: string): Promise<string> {
        return this.inner.readFileAsString(this.state.resolve(path));
    }

    readFileAsBytes(path: string): Promise<Uint8Array> {
        return this.inner.readFileAsBytes(this.state.resolve(path));
    }

    writeStringToFile(path: string, content: string): Promise<void> {
        return this.inner.writeStringToFile(this.state.resolve(path), content);
    }

    writeBytesToFile(path: string, content: Uint8Array): Promise<void> {
        return this.inner.writeBytesToFile(this.state.resolve(path), content);
    }

    pathExists(path: string): Promise<boolean> {
        return this.inner.pathExists(this.state.resolve(path));
    }

    createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        return this.inner.createDirectory(this.state.resolve(path), options);
    }

    readDirectory(path: string): Promise<string[]> {
        return this.inner.readDirectory(this.state.resolve(path));
    }

    remove(path: string, options?: { recursive?: boolean }): Promise<void> {
        return this.inner.remove(this.state.resolve(path), options);
    }

    rename(oldPath: string, newPath: string): Promise<void> {
        return this.inner.rename(
            this.state.resolve(oldPath),
            this.state.resolve(newPath),
        );
    }

    stat(path: string): Promise<FileStat> {
        return this.inner.stat(this.state.resolve(path));
    }

    isFile(path: string): Promise<boolean> {
        return this.inner.isFile(this.state.resolve(path));
    }

    isDirectory(path: string): Promise<boolean> {
        return this.inner.isDirectory(this.state.resolve(path));
    }

    isSymbolicLink(path: string): Promise<boolean> {
        return this.inner.isSymbolicLink(this.state.resolve(path));
    }

    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        return this.inner.copy(
            this.state.resolve(src),
            this.state.resolve(dest),
            options,
        );
    }

    appendStringToFile(path: string, content: string): Promise<void> {
        return this.inner.appendStringToFile(this.state.resolve(path), content);
    }
}

/* ------------------------------------------------------------------------- */
/* InMemoryCwdProcess                                                         */
/* ------------------------------------------------------------------------- */

/**
 * `Process` wrapper that virtualises `currentDir()` / `setCurrentDir()`.
 *
 * `currentDir()` reflects the in-memory directory and `setCurrentDir()`
 * updates it without touching the wrapped process, while `args()` and `env()`
 * continue to delegate to the inner process.
 */
export class InMemoryCwdProcess implements Process {
    constructor(
        private readonly inner: Process,
        private readonly state: InMemoryCwdState,
    ) {}

    currentDir(): string {
        return this.state.current;
    }

    setCurrentDir(dir: string): Promise<void> {
        // Intercepted: tracked purely in memory, never forwarded to the
        // wrapped (host) process.
        this.state.set(dir);
        return Promise.resolve();
    }

    args(): ArgsList {
        return this.inner.args();
    }

    env(): Env {
        return this.inner.env();
    }
}
