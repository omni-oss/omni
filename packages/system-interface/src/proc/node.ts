import process from "node:process";
import type { Process, ProcessEnv } from ".";

export class NodeProcess implements Process {
    currentDir(): string {
        return process.cwd();
    }

    setCurrentDir(dir: string): void {
        process.chdir(dir);
    }

    args(): string[] {
        return process.argv;
    }

    env(): ProcessEnv {
        return process.env;
    }
}
