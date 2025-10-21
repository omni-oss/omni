import { Dirent } from "node:fs";
import { resolve } from "node:path";
import type { IFs, Volume } from "memfs";
import type { Process } from "@/proc";
import { promisify, promisifyNoErr } from "./helper";
import type { FileStat, FileSystem } from "./interfaces";

export class VirtualFileSystem implements FileSystem {
    private fs: ReturnType<typeof promisifyMemFs>;

    constructor(
        mem: {
            fs: IFs;
            vol: Volume;
        },
        private proc: Process,
    ) {
        this.fs = promisifyMemFs(mem);
    }

    private resolve(path: string): string {
        return resolve(this.proc.currentDir(), path);
    }

    async readFileAsString(path: string): Promise<string> {
        const data = await this.fs.readFile(this.resolve(path), {
            encoding: "utf-8",
        });
        return data.toString();
    }

    writeStringToFile(path: string, content: string): Promise<void> {
        const data = Buffer.from(content, "utf-8");
        return this.fs.writeFile(this.resolve(path), data, {
            encoding: "utf-8",
        });
    }

    pathExists(path: string): Promise<boolean> {
        return this.fs.exists(this.resolve(path));
    }

    async createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        await this.fs.mkdir(this.resolve(path), {
            recursive: options?.recursive ?? false,
        });
    }

    async readDirectory(path: string): Promise<string[]> {
        const files = await this.fs.readdir(this.resolve(path), {});

        return files.map((file) => {
            if (file instanceof Dirent) {
                return file.name;
            } else {
                return file;
            }
        });
    }

    remove(path: string, options?: { recursive?: boolean }): Promise<void> {
        return this.fs.rm(this.resolve(path), {
            recursive: options?.recursive ?? false,
        });
    }

    rename(oldPath: string, newPath: string): Promise<void> {
        return this.fs.rename(oldPath, newPath);
    }

    async stat(path: string): Promise<FileStat> {
        const stat = await this.fs.stat(this.resolve(path), {
            throwIfNoEntry: true,
            bigint: false,
        });

        return {
            isDirectory: () => stat.isDirectory(),
            isFile: () => stat.isFile(),
            isSymbolicLink: () => stat.isSymbolicLink(),
            mtime: stat.mtime,
            size: Number(stat.size),
        };
    }

    async isFile(path: string): Promise<boolean> {
        const stat = await this.stat(this.resolve(path));
        return stat.isFile();
    }

    async isDirectory(path: string): Promise<boolean> {
        const stat = await this.stat(this.resolve(path));
        return stat.isDirectory();
    }

    async isSymbolicLink(path: string): Promise<boolean> {
        const stat = await this.stat(this.resolve(path));
        return stat.isSymbolicLink();
    }

    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        return this.fs.cp(this.resolve(src), this.resolve(dest), {
            recursive: options?.recursive ?? false,
            force: options?.overwrite ?? false,
        });
    }

    appendStringToFile(path: string, content: string): Promise<void> {
        return this.fs.appendFile(this.resolve(path), content, {
            encoding: "utf-8",
        });
    }
}

function promisifyMemFs(mem: { fs: IFs; vol: Volume }) {
    return {
        mem,
        readFile: promisify(mem.fs.readFile),
        writeFile: promisify(mem.fs.writeFile),
        exists: promisifyNoErr(mem.fs.exists),
        mkdir: promisify(mem.fs.mkdir),
        readdir: promisify(mem.fs.readdir),
        rm: promisify(mem.fs.rm),
        rename: promisify(mem.fs.rename),
        cp: promisify(mem.fs.cp),
        stat: promisify(mem.fs.stat),
        appendFile: promisify(mem.fs.appendFile),
    };
}
