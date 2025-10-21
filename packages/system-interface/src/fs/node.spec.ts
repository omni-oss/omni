import { NodeFileSystem } from "./node";
import { declareFsTests } from "./test-declarations";

declareFsTests({
    fs: new NodeFileSystem(),
    name: "Node",
});
