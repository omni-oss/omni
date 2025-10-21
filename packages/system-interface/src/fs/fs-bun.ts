import { throwIfBunNotAvailable } from "@/platform-helpers";
import type { FileStat, FileSystem } from ".";
import { NodeFileSystem } from "./fs-node";

export class BunFileSystem implements FileSystem {
    private fallbackFs: NodeFileSystem = new NodeFileSystem();

    readFileAsString(path: string): Promise<string> {
        throwIfBunNotAvailable();

        return Bun.file(path).text();
    }

    async writeStringToFile(path: string, content: string): Promise<void> {
        throwIfBunNotAvailable();

        await Bun.file(path).write(content);
    }

    pathExists(path: string): Promise<boolean> {
        throwIfBunNotAvailable();

        return Bun.file(path).exists();
    }

    createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        return this.fallbackFs.createDirectory(path, options);
    }

    readDirectory(path: string): Promise<string[]> {
        return this.fallbackFs.readDirectory(path);
    }

    remove(path: string, options?: { recursive?: boolean }): Promise<void> {
        return this.fallbackFs.remove(path, options);
    }

    rename(oldPath: string, newPath: string): Promise<void> {
        return this.fallbackFs.rename(oldPath, newPath);
    }

    stat(path: string): Promise<FileStat> {
        return Bun.file(path).stat();
    }

    async isFile(path: string): Promise<boolean> {
        const stat = await Bun.file(path).stat();
        return stat.isFile();
    }

    async isDirectory(path: string): Promise<boolean> {
        const stat = await Bun.file(path).stat();
        return stat.isDirectory();
    }

    async isSymbolicLink(path: string): Promise<boolean> {
        const stat = await Bun.file(path).stat();
        return stat.isSymbolicLink();
    }

    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        return this.fallbackFs.copy(src, dest, options);
    }

    appendStringToFile(path: string, content: string): Promise<void> {
        return this.fallbackFs.appendStringToFile(path, content);
    }
}
