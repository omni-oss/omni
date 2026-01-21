/**
 * Represents specific error conditions that can occur during a Response lifecycle.
 */
export class ResponseErrorCode {
    public static readonly UNEXPECTED_FRAME = new ResponseErrorCode(0);

    constructor(private readonly _value: number) {}

    public toString(): string {
        return this._value.toString();
    }

    public toJSON(): number {
        return this._value;
    }

    public valueOf(): number {
        return this._value;
    }
}

/**
 * Represents specific error conditions that can occur during a Request lifecycle.
 */
export class RequestErrorCode {
    public static readonly UNEXPECTED_FRAME = new RequestErrorCode(0);
    public static readonly TIMED_OUT = new RequestErrorCode(1);

    constructor(private readonly _value: number) {}

    public toString(): string {
        return this._value.toString();
    }

    public toJSON(): number {
        return this._value;
    }

    public valueOf(): number {
        return this._value;
    }
}
