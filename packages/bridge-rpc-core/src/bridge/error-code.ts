import { getOrSet } from "./code-utils";
import { Uint16Schema } from "./int-schema";

/**
 * Represents specific error conditions that can occur during a Response lifecycle.
 */
export class ResponseErrorCode {
    public static readonly UNEXPECTED_FRAME = new ResponseErrorCode(0);
    public static readonly INTERNAL = new ResponseErrorCode(1);
    private static readonly _customCodes = new Map<number, ResponseErrorCode>();

    private constructor(private readonly _value: number) {
        _value = Uint16Schema.parse(_value);
    }

    public static from(value: number): ResponseErrorCode {
        switch (value) {
            case 0:
                return ResponseErrorCode.UNEXPECTED_FRAME;
            case 1:
                return ResponseErrorCode.INTERNAL;
            default: {
                return getOrSet(
                    ResponseErrorCode._customCodes,
                    value,
                    () => new ResponseErrorCode(value),
                );
            }
        }
    }

    public toString(): string {
        return this._value.toString();
    }

    public toJSON(): number {
        return this._value;
    }

    public valueOf(): number {
        return this._value;
    }

    public equals(other: ResponseErrorCode): boolean {
        return this._value === other._value;
    }
}

export const ResponseErrorCodeConstructor = ResponseErrorCode as unknown as {
    new (value: number): ResponseErrorCode;
};

/**
 * Represents specific error conditions that can occur during a Request lifecycle.
 */
export class RequestErrorCode {
    public static readonly UNEXPECTED_FRAME = new RequestErrorCode(0);
    public static readonly TIMED_OUT = new RequestErrorCode(1);
    private static readonly _customCodes = new Map<number, RequestErrorCode>();

    private constructor(private readonly _value: number) {
        _value = Uint16Schema.parse(_value);
    }

    public static from(value: number): RequestErrorCode {
        switch (value) {
            case 0:
                return RequestErrorCode.UNEXPECTED_FRAME;
            case 1:
                return RequestErrorCode.TIMED_OUT;
            default: {
                return getOrSet(
                    RequestErrorCode._customCodes,
                    value,
                    () => new RequestErrorCode(value),
                );
            }
        }
    }

    public toString(): string {
        return this._value.toString();
    }

    public toJSON(): number {
        return this._value;
    }

    public valueOf(): number {
        return this._value;
    }

    public equals(other: RequestErrorCode): boolean {
        return this._value === other._value;
    }
}

export const RequestErrorCodeConstructor = RequestErrorCode as unknown as {
    new (value: number): RequestErrorCode;
};
