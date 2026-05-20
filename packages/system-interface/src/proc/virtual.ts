import type { Process, ProcessEnv } from "./interfaces";

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

    args(): string[] {
        return this.argsValues;
    }

    env(): ProcessEnv {
        return this.envVars;
    }
}
