import { Dirent } from "node:fs";
import type { IFs, Volume } from "memfs";
import type { FileStat, FileSystem } from "./fs-interfaces";

export class VirtualFileSystem implements FileSystem {
    private fs: ReturnType<typeof promisifyMemFs>;

    constructor(mem: {
        fs: IFs;
        vol: Volume;
    }) {
        this.fs = promisifyMemFs(mem);
    }

    async readFileAsString(path: string): Promise<string> {
        const data = await this.fs.readFile(path, { encoding: "utf-8" });
        return data.toString();
    }

    writeStringToFile(path: string, content: string): Promise<void> {
        const data = Buffer.from(content, "utf-8");
        return this.fs.writeFile(path, data, { encoding: "utf-8" });
    }

    pathExists(path: string): Promise<boolean> {
        return this.fs.exists(path);
    }

    async createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void> {
        await this.fs.mkdir(path, {
            recursive: options?.recursive ?? false,
        });
    }

    async readDirectory(path: string): Promise<string[]> {
        const files = await this.fs.readdir(path, {});

        return files.map((file) => {
            if (file instanceof Dirent) {
                return file.name;
            } else {
                return file;
            }
        });
    }

    remove(path: string, options?: { recursive?: boolean }): Promise<void> {
        return this.fs.rm(path, {
            recursive: options?.recursive ?? false,
        });
    }

    rename(oldPath: string, newPath: string): Promise<void> {
        return this.fs.rename(oldPath, newPath);
    }

    async stat(path: string): Promise<FileStat> {
        const stat = await this.fs.stat(path, {
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

    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void> {
        return this.fs.cp(src, dest, {
            recursive: options?.recursive ?? false,
            force: options?.overwrite ?? false,
        });
    }

    appendStringToFile(path: string, content: string): Promise<void> {
        return this.fs.appendFile(path, content, {
            encoding: "utf-8",
        });
    }
}

// biome-ignore lint/suspicious/noExplicitAny: generic
function promisify<TArgs extends any[], TError, TResult>(
    fn: (...args: [...TArgs, (error: TError, res?: TResult) => void]) => void,
) {
    return (...args: TArgs): Promise<TResult> => {
        return new Promise((resolve, reject) => {
            fn(...args, (err, result) => {
                if (err) reject(err);
                else resolve(result as TResult);
            });
        });
    };
}

// biome-ignore lint/suspicious/noExplicitAny: generic
function promisifyNoErr<TArgs extends any[], TResult>(
    fn: (...args: [...TArgs, (res: TResult) => void]) => void,
) {
    return (...args: TArgs): Promise<TResult> => {
        return new Promise((resolve, _reject) => {
            fn(...args, (result) => {
                return resolve(result as TResult);
            });
        });
    };
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
