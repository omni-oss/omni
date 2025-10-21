import { memfs } from "memfs";
import { VirtualFileSystem } from "@/fs/virtual";
import { VirtualProcess } from "@/proc/virtual";
import type { System } from "./interfaces";

export class VirtualSystem implements System {
    constructor(
        public fs: VirtualFileSystem,
        public proc: VirtualProcess,
    ) {}

    public static async create(): Promise<VirtualSystem> {
        const process = await VirtualProcess.create();
        return new VirtualSystem(
            new VirtualFileSystem(memfs(), process),
            process,
        );
    }
}
