import { ObjectEnv } from "./env";
import type { ArgsList, Env, Process, ProcessEnv } from "./interfaces";

export class VirtualProcess implements Process {
    constructor(
        private cwd: string,
        private readonly argsValues: string[],
        private readonly envVars: ProcessEnv,
    ) {}

    static async create(): Promise<VirtualProcess> {
        return new VirtualProcess("/", [], {});
    }

    currentDir(): string {
        return this.cwd;
    }

    setCurrentDir(dir: string): Promise<void> {
        this.cwd = dir;
        return Promise.resolve();
    }

    args(): ArgsList {
        return this.argsValues;
    }

    env(): Env {
        return new ObjectEnv(this.envVars);
    }
}
