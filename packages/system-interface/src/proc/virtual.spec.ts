import fs from "node:fs";
import os from "node:os";
import { declareProcTests } from "./test-declarations";
import { VirtualProcess } from "./virtual";

declareProcTests({
    name: "Deno",
    args: process.argv,
    currentDir: fs.realpathSync(process.cwd()),
    env: process.env,
    newCurrentDir: fs.realpathSync(os.tmpdir()),
    proc: new VirtualProcess(process.cwd(), process.argv, process.env),
});
