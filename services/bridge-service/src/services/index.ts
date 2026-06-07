import type { Handler, Service } from "@omni-oss/bridge-rpc-bootstrap";
import { ExecGeneratorScript } from "@omni-oss/bridge-rpc-services/exec-generator-script";

export function createServices(): (Handler | Service)[] {
    return [
        {
            path: "/exec-generator-script",
            service: new ExecGeneratorScript(),
        },
    ];
}
