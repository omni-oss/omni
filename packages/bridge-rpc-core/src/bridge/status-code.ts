import { Uint16Schema } from "./int-schema";

export class ResponseStatusCode {
    public static readonly SUCCESS = new ResponseStatusCode(0);
    public static readonly NO_HANDLER_FOR_PATH = new ResponseStatusCode(100);

    constructor(private readonly _value: number) {
        _value = Uint16Schema.parse(_value);
    }

    public static from(value: number): ResponseStatusCode {
        switch (value) {
            case 0:
                return ResponseStatusCode.SUCCESS;
            case 100:
                return ResponseStatusCode.NO_HANDLER_FOR_PATH;
            default:
                return new ResponseStatusCode(value);
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

    public equals(other: ResponseStatusCode): boolean {
        return this._value === other._value;
    }
}
