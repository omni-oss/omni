import type { FileStat, FileSystem } from "./interfaces";

async function getFs(): Promise<FileSystem> {
    if (typeof Deno !== "undefined") {
        return new (await import("./deno")).DenoFileSystem();
    }

    if (typeof Bun !== "undefined") {
        return new (await import("./bun")).BunFileSystem();
    }

    return new (await import("./node")).NodeFileSystem();
}

/**
 * This is a wrapper around the FileSystem interface that uses the best available implementation.
 */
export class OptimizedFileSystem implements FileSystem {
    private constructor(private fs: FileSystem) {}

    static async create(): Promise<OptimizedFileSystem> {
        return new OptimizedFileSystem(await getFs());
    }

    readFileAsString(path: string): Promise<string> {
        return this.fs.readFileAsString(path);
    }

    writeStringToFile(path: string, content: string): Promise<void> {
        return this.fs.writeStringToFile(path, content);
    }

    pathExists(path: string): Promise<boolean> {
        return this.fs.pathExists(path);
    }

    createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        return this.fs.createDirectory(path, options);
    }

    readDirectory(path: string): Promise<string[]> {
        return this.fs.readDirectory(path);
    }

    remove(path: string, options?: { recursive?: boolean }): Promise<void> {
        return this.fs.remove(path, options);
    }

    rename(oldPath: string, newPath: string): Promise<void> {
        return this.fs.rename(oldPath, newPath);
    }

    stat(path: string): Promise<FileStat> {
        return this.fs.stat(path);
    }

    isFile(path: string): Promise<boolean> {
        return this.fs.isFile(path);
    }

    isDirectory(path: string): Promise<boolean> {
        return this.fs.isDirectory(path);
    }

    isSymbolicLink(path: string): Promise<boolean> {
        return this.fs.isSymbolicLink(path);
    }

    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        return this.fs.copy(src, dest, options);
    }

    appendStringToFile(path: string, content: string): Promise<void> {
        return this.fs.appendStringToFile(path, content);
    }
}
