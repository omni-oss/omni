import process from "node:process";
import type { ArgsList, Env, Process } from ".";
import { ObjectEnv } from "./env";

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

    env(): Env {
        return new ObjectEnv(process.env);
    }
}
