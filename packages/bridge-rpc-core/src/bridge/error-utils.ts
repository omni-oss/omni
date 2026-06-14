import type { OneshotReceiver } from "@omni-oss/channels";

export function errorFromFrame<
    T extends {
        message: string;
        code: {
            valueOf(): number;
        };
    },
>(frame: T): Error {
    return new Error(
        `${frame.message} (status code: ${frame.code.valueOf()})`,
        {
            cause: frame,
        },
    );
}

export async function throwIfError<
    T extends { message: string; code: { valueOf(): number } },
>(receiver: OneshotReceiver<T>): Promise<void> {
    if (receiver.hasValue()) {
        const frame = await receiver.receive();
        if (frame) {
            throw errorFromFrame(frame);
        }
    }
}
