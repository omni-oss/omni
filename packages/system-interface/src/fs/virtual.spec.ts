import { memfs } from "memfs";
import { declareFsTests } from "./test-declarations";
import { VirtualFileSystem } from "./virtual";

declareFsTests({
    fs: new VirtualFileSystem(memfs()),
    name: "Virtual",
    useRealDir: false,
});
