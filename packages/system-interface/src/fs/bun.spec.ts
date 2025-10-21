import { BunFileSystem } from "./bun";
import { declareFsTests } from "./test-declarations";

declareFsTests({
    fs: new BunFileSystem(),
    name: "Bun",
    skip: typeof Bun === "undefined",
});
