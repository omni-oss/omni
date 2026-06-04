import { Command } from "@commander-js/extra-typings";
import { createStudioRpcInstance } from "@/index";
import { interceptLogs } from "@/log-interceptor";
import { description, name, version } from "../../package.json";

const program = new Command();

const rpc = createStudioRpcInstance();

program.name(name).version(version).description(description);

program.command("run").action(async () => {
    const { logs: _l, result: _r } = await interceptLogs(() => rpc.start());
});

program.parseAsync();
