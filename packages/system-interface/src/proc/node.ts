import process from "node:process";
import type { ArgsList, Process, ProcessEnv } from ".";

export class NodeProcess implements Process {
    currentDir(): string {
        return process.cwd();
    }

    setCurrentDir(dir: string): Promise<void> {
        process.chdir(dir);
        return Promise.resolve();
    }

    args(): ArgsList {
        return process.argv;
    }

    env(): ProcessEnv {
        return process.env;
    }
}
