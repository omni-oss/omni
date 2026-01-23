import type { MpscSender } from "@/mpsc";
import { encode } from "./codec-utils";

export async function sendToMpsc<T>(
    mpscSender: MpscSender<Uint8Array>,
    data: T,
) {
    await mpscSender.send(encode(data));
}
