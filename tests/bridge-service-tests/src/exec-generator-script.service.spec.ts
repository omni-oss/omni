import { join } from "node:path";
import { ResponseStatusCode } from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { describe, expect, it } from "vitest";
import { json, TEXT } from "./helpers";

describe("/exec-generator-script", {
    timeout: 10_000,
}, () => {
    it("should respond to requests", async () => {
        const request = await TsRpcClient.request(
            "/exec-generator-script",
        ).then((req) => req.start());
        const scriptPath = join(__dirname, "__fixtures__", "test.mjs");
        await request.writeBodyChunk(
            json([
                {
                    path: scriptPath,
                    params: {
                        dry_run: true,
                        data: { greeting: "hi" },
                    },
                },
            ]),
        );
        const end = await request.end().then((x) => x.wait());

        const body = await readBody(end);
        if (!end.status.equals(ResponseStatusCode.SUCCESS)) {
            console.error("Error response body:", TEXT.decode(body));
        }

        expect(end.status).toEqual(ResponseStatusCode.SUCCESS);
    });
});
