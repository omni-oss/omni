import type { MpscSender } from "@/mpsc";
import type { OneshotSender } from "@/oneshot";
import type { ResponseFrameEvent } from "./client/response";
import type { RequestError, ResponseError, ResponseStart } from "./frame";
import type { RequestFrameEvent } from "./server/request";

export class RequestSessionContext {
    constructor(
        public readonly requestFrameSender: MpscSender<RequestFrameEvent>,
        public readonly requestErrorSender: OneshotSender<RequestError>,
    ) {}
}

export class ResponseSessionContext {
    constructor(
        public readonly responseStartSender: OneshotSender<ResponseStart>,
        public readonly responseFrameSender: MpscSender<ResponseFrameEvent>,
        public readonly responseErrorSender: OneshotSender<ResponseError>,
    ) {}
}
