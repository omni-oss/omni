import type { MpscSender, OneshotSender } from "@omni-oss/channels";
import type { ResponseFrameEvent } from "./client/response";
import type { RequestError, ResponseError, ResponseStart } from "./frame";
import type { RequestFrameEvent } from "./server/request";
import type { ClosableSessionContext } from "./session/session-context";

export class RequestSessionContext implements ClosableSessionContext {
    constructor(
        public readonly requestFrameSender: MpscSender<
            RequestFrameEvent,
            number | undefined
        >,
        public readonly requestErrorSender: OneshotSender<RequestError>,
    ) {}

    /**
     * Releases the underlying channels when the session is closed so that a
     * consumer blocked reading the request body observes EOF instead of
     * hanging forever. This mirrors the Rust implementation, where dropping
     * the senders on close terminates the corresponding receivers.
     */
    close() {
        this.requestFrameSender.close();
    }
}

export class ResponseSessionContext implements ClosableSessionContext {
    constructor(
        public readonly responseStartSender: OneshotSender<ResponseStart>,
        public readonly responseFrameSender: MpscSender<
            ResponseFrameEvent,
            number | undefined
        >,
        public readonly responseErrorSender: OneshotSender<ResponseError>,
    ) {}

    /**
     * Releases the underlying channels when the session is closed so that a
     * pending `wait()` or body reader is unblocked instead of hanging
     * forever (for example when the peer sends a response error frame).
     * This mirrors the Rust implementation, where dropping the senders on
     * close terminates the corresponding receivers.
     */
    close() {
        // Unblock a pending `wait()` if the response never started.
        if (
            !this.responseStartSender.isSent() &&
            !this.responseStartSender.isClosed()
        ) {
            this.responseStartSender.close();
        }
        // Unblock a body reader if the response is torn down mid-stream.
        this.responseFrameSender.close();
    }
}
