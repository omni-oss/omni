import { type FileSystem, OptimizedFileSystem } from "@/fs";
import type { Process } from "@/proc";
import { OptimizedProcess } from "@/proc";
import type { System } from "./interfaces";

export class OptimizedSystem implements System {
    private constructor(
        public fs: FileSystem,
        public proc: Process,
    ) {}

    static async create(): Promise<OptimizedSystem> {
        return new OptimizedSystem(
            await OptimizedFileSystem.create(),
            await OptimizedProcess.create(),
        );
    }
}
