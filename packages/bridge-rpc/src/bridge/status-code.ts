export class ResponseStatusCode {
    public static readonly SUCCESS = new ResponseStatusCode(0);
    public static readonly NO_HANDLER_FOR_PATH = new ResponseStatusCode(100);

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
