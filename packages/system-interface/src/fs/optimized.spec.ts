import { OptimizedFileSystem } from "./optimized";
import { declareFsTests } from "./test-declarations";

declareFsTests({
    fs: await OptimizedFileSystem.create(),
    name: "Optimized",
});
