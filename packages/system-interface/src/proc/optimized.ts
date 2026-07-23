import type { ArgsList, Env, Process } from "./interfaces";

async function getProc(): Promise<Process> {
    if (typeof Deno !== "undefined") {
        return new (await import("./deno")).DenoProcess();
    }

    if (typeof Bun !== "undefined") {
        return new (await import("./bun")).BunProcess();
    }

    return new (await import("./node")).NodeProcess();
}

export class OptimizedProcess implements Process {
    private constructor(private proc: Process) {}

    static async create(): Promise<OptimizedProcess> {
        return new OptimizedProcess(await getProc());
    }

    currentDir(): string {
        return this.proc.currentDir();
    }

    setCurrentDir(dir: string): Promise<void> {
        return this.proc.setCurrentDir(dir);
    }

    args(): ArgsList {
        return this.proc.args();
    }

    env(): Env {
        return this.proc.env();
    }
}
