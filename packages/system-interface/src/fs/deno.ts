import { throwIfDenoNotAvailable } from "@/platform-helpers";
import type { FileStat, FileSystem } from "./interfaces";
import { NodeFileSystem } from "./node";

export class DenoFileSystem implements FileSystem {
    private fallbackFs: NodeFileSystem = new NodeFileSystem();

    async readFileAsString(path: string): Promise<string> {
        throwIfDenoNotAvailable();
        return Deno.readTextFile(path);
    }

    async writeStringToFile(path: string, content: string): Promise<void> {
        throwIfDenoNotAvailable();
        await Deno.writeTextFile(path, content);
    }

    async pathExists(path: string): Promise<boolean> {
        throwIfDenoNotAvailable();

        try {
            await Deno.lstat(path);
            return true;
        } catch (e) {
            if (e instanceof Deno.errors.NotFound) {
                return false;
            }
            throw e;
        }
    }

    createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        throwIfDenoNotAvailable();

        return Deno.mkdir(path, { recursive: options?.recursive ?? false });
    }

    async readDirectory(path: string): Promise<string[]> {
        throwIfDenoNotAvailable();

        const dir = Deno.readDir(path);
        const files: string[] = [];

        for await (const f of dir) {
            files.push(f.name);
        }

        return files;
    }

    remove(path: string, options?: { recursive?: boolean }): Promise<void> {
        throwIfDenoNotAvailable();

        return Deno.remove(path, { recursive: options?.recursive ?? false });
    }

    rename(oldPath: string, newPath: string): Promise<void> {
        throwIfDenoNotAvailable();

        return Deno.rename(oldPath, newPath);
    }

    async stat(path: string): Promise<FileStat> {
        throwIfDenoNotAvailable();

        const c = await Deno.lstat(path);
        return DenoFileStat.fromDenoFileInfo(c);
    }

    async isFile(path: string): Promise<boolean> {
        return this.stat(path).then((stat) => stat.isFile());
    }

    async isDirectory(path: string): Promise<boolean> {
        return this.stat(path).then((stat) => stat.isDirectory());
    }

    async isSymbolicLink(path: string): Promise<boolean> {
        return this.stat(path).then((stat) => stat.isSymbolicLink());
    }

    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        return this.fallbackFs.copy(src, dest, options);
    }

    appendStringToFile(path: string, content: string): Promise<void> {
        throwIfDenoNotAvailable();
        return Deno.writeTextFile(path, content, { append: true });
    }
}

class DenoFileStat implements FileStat {
    private constructor(private stat: Deno.FileInfo) {}

    isFile(): boolean {
        return this.stat.isFile;
    }

    isDirectory(): boolean {
        return this.stat.isDirectory;
    }

    isSymbolicLink(): boolean {
        return this.stat.isSymlink;
    }

    get mtime(): Date {
        const mtime = this.stat.mtime;

        if (!mtime) {
            throw new Error("mtime is not available");
        }

        return mtime;
    }

    get size(): number {
        return this.stat.size;
    }

    static fromDenoFileInfo(info: Deno.FileInfo): DenoFileStat {
        return new DenoFileStat(info);
    }
}
