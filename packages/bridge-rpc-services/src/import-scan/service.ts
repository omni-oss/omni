import {
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import { readBodyAsJson } from "@omni-oss/bridge-rpc-utils/body";
import { fail } from "@omni-oss/bridge-rpc-utils/server";
import { RUNTIME } from "@omni-oss/runtime-utils";
import z from "zod";
import { type ClosureResult, computeClosure } from "./closure";
import { computeDenoClosure } from "./deno-graph";
import { makeResolver } from "./resolvers";

const STATUS_BAD_REQUEST = ResponseStatusCode.from(400);
const STATUS_INTERNAL_ERROR = ResponseStatusCode.from(500);
const TEXT_ENCODER = new TextEncoder();

const ImportScanPayloadSchema = z.object({
    /** The generator entry scripts to compute the read closure for. */
    entries: z.array(z.string()).min(1),
});

export type ImportScanPayload = z.infer<typeof ImportScanPayloadSchema>;

/**
 * Computes the runtime-authoritative import closure of a set of generator
 * entries so the host can grant a confined child read access to exactly the
 * files it will load — no more. It runs UNCONFINED and executes none of the
 * scanned code: strategy C (Node/Bun) statically lexes specifiers and resolves
 * them with the runtime's own resolver; strategy B (Deno) reads the runtime's
 * `deno info --json` graph. Both return a bounded file closure plus
 * `node_modules`/npm package roots as grant boundaries.
 */
export class ImportScan implements Service {
    public async run(context: ServiceContext): Promise<void> {
        let payload: ImportScanPayload;
        try {
            const body = await readBodyAsJson<unknown>(context.request);
            payload = ImportScanPayloadSchema.parse(body);
        } catch (err) {
            await fail(context.response, STATUS_BAD_REQUEST, err);
            return;
        }

        let result: ClosureResult;
        try {
            result =
                RUNTIME === "deno"
                    ? await computeDenoClosure(payload.entries)
                    : await computeClosure({
                          entries: payload.entries,
                          resolve: makeResolver(),
                      });
        } catch (err) {
            await fail(context.response, STATUS_INTERNAL_ERROR, err);
            return;
        }

        const response = await context.response.start(
            ResponseStatusCode.SUCCESS,
        );
        await response.writeBodyChunk(
            TEXT_ENCODER.encode(JSON.stringify(result)),
        );
        await response.end();
    }
}
