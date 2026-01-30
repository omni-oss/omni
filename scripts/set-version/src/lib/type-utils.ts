// Helper to create an array of a specific length
export type StringToNumber<S extends string> =
    S extends `${infer N extends number}` ? N : never;

export type Primitive =
    | string
    | number
    | boolean
    | bigint
    | symbol
    | null
    | undefined;

export type IsTuple<T> = T extends readonly [...infer _]
    ? number extends T["length"]
        ? false
        : true
    : false;

export type TupleKeys<T extends readonly unknown[]> = {
    [K in keyof T]: K extends string ? StringToNumber<K> : K;
}[number];
