import type { Process, ProcessEnv } from "./proc-interfaces";

async function getProc(): Promise<Process> {
    if (typeof Deno !== "undefined") {
        return new (await import("./proc-deno")).DenoProcess();
    }

    if (typeof Bun !== "undefined") {
        return new (await import("./proc-bun")).BunProcess();
    }

    return new (await import("./proc-node")).NodeProcess();
}

export class OptimizedProcess implements Process {
    private constructor(private proc: Process) {}

    static async create(): Promise<OptimizedProcess> {
        return new OptimizedProcess(await getProc());
    }

    currentDir(): string {
        return this.proc.currentDir();
    }

    setCurrentDir(dir: string): void {
        this.proc.setCurrentDir(dir);
    }

    args(): string[] {
        return this.proc.args();
    }

    env(): ProcessEnv {
        return this.proc.env();
    }
}
