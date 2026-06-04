import type { Handler, Service } from "@omni-oss/bridge-rpc-bootstrap";
import { ExecScript } from "@omni-oss/bridge-rpc-services/exec-script";
import { interceptLogs } from "@/log-interceptor";

export function createServices(): (Handler | Service)[] {
    return [
        {
            path: "/exec-script",
            service: new ExecScript({
                import: async (spec, forward) => {
                    const { logs: _, result } = await interceptLogs(() =>
                        forward(spec),
                    );

                    return result;
                },
            }),
        },
    ];
}
