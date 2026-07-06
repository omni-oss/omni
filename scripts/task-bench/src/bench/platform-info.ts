import os from "node:os";
export interface CpuHardwareInfo {
    model: string;
    speedMHz: number;
}

export interface PlatformInfo {
    cpus: CpuHardwareInfo[];
    memory: {
        totalBytes: number;
        freeBytes: number;
    };
    os: {
        platform: string;
        release: string;
        arch: string;
    };
}

export function getPlatformInfo(): PlatformInfo {
    return {
        cpus: os.cpus().map((cpu) => ({
            model: cpu.model,
            speedMHz: cpu.speed,
        })),
        memory: {
            totalBytes: os.totalmem(),
            freeBytes: os.freemem(),
        },
        os: {
            platform: os.platform(),
            release: os.release(),
            arch: os.arch(),
        },
    };
}
