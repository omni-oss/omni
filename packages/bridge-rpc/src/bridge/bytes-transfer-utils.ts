import type { MpscSender } from "@omni-oss/channels";
import { encode } from "./codec-utils";

export async function sendToMpsc<T>(
    mpscSender: MpscSender<Uint8Array>,
    data: T,
) {
    mpscSender.send(encode(data));
}
