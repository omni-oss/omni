import { memfs } from "memfs";
import { VirtualProcess } from "@/proc/virtual";
import { declareFsTests } from "./test-declarations";
import { VirtualFileSystem } from "./virtual";

declareFsTests({
    fs: new VirtualFileSystem(memfs(), await VirtualProcess.create()),
    name: "Virtual",
    useRealDir: false,
});
