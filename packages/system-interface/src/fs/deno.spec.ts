import { DenoFileSystem } from "./deno";
import { declareFsTests } from "./test-declarations";

declareFsTests({
    fs: new DenoFileSystem(),
    name: "Deno",
    skip: typeof Deno === "undefined",
});
