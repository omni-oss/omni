import { loadNodeFs } from "./helper";
import type { FileStat, FileSystem } from "./interfaces";

export class NodeFileSystem implements FileSystem {
    async readFileAsString(path: string): Promise<string> {
        const fs = await loadNodeFs();
        return fs.readFile(path, "utf-8");
    }

    async readFileAsBytes(path: string): Promise<Uint8Array> {
        const fs = await loadNodeFs();
        return fs.readFile(path);
    }

    async writeStringToFile(path: string, content: string): Promise<void> {
        const fs = await loadNodeFs();
        await fs.writeFile(path, content, "utf-8");
    }

    async writeBytesToFile(path: string, content: Uint8Array): Promise<void> {
        const fs = await loadNodeFs();
        await fs.writeFile(path, content);
    }

    async pathExists(path: string): Promise<boolean> {
        const fs = await loadNodeFs();

        return fs.exists(path);
    }

    async createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        const fs = await loadNodeFs();

        await fs.mkdir(path, { recursive: options?.recursive ?? false });
    }

    async readDirectory(path: string): Promise<string[]> {
        const fs = await loadNodeFs();

        return fs.readdir(path);
    }

    async remove(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        const fs = await loadNodeFs();

        return fs.rm(path, { recursive: options?.recursive ?? false });
    }

    async rename(oldPath: string, newPath: string): Promise<void> {
        const fs = await loadNodeFs();

        return fs.rename(oldPath, newPath);
    }

    async stat(path: string): Promise<FileStat> {
        const fs = await loadNodeFs();

        return fs.stat(path);
    }

    async isFile(path: string): Promise<boolean> {
        const stat = await this.stat(path);
        return stat.isFile();
    }

    async isDirectory(path: string): Promise<boolean> {
        const stat = await this.stat(path);
        return stat.isDirectory();
    }

    async isSymbolicLink(path: string): Promise<boolean> {
        const stat = await this.stat(path);
        return stat.isSymbolicLink();
    }

    async copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        const fs = await loadNodeFs();

        return fs.cp(src, dest, {
            recursive: options?.recursive ?? false,
            force: options?.overwrite ?? false,
        });
    }

    async appendStringToFile(path: string, content: string): Promise<void> {
        const fs = await loadNodeFs();

        return fs.appendFile(path, content);
    }
}
