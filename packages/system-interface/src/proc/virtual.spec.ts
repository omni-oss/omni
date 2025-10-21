import os from "node:os";
import { declareProcTests } from "./test-declarations";
import { VirtualProcess } from "./virtual";

declareProcTests({
    name: "Deno",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: new VirtualProcess(process.cwd(), process.argv, process.env),
});
