import type { IsTuple, Primitive, TupleKeys } from "./type-utils";

type Path<T> =
    | (T extends Primitive
          ? []
          : T extends readonly (infer U)[]
            ? [TupleKeys<T>, ...Path<U>]
            : {
                  [K in keyof T & (string | number)]: [K] | [K, ...Path<T[K]>];
              }[keyof T & (string | number)])
    | [];

type SetAtPath<T, P extends readonly unknown[], V> = P extends []
    ? V
    : P extends [infer H, ...infer R]
      ? T extends readonly unknown[]
          ? IsTuple<T> extends true
              ? {
                    [K in keyof T]: K extends `${H & number}`
                        ? SetAtPath<T[K], R, V>
                        : T[K];
                }
              : T extends readonly (infer U)[]
                ? readonly SetAtPath<U, R, V>[]
                : T
          : H extends keyof T
            ? Omit<T, H> & { [K in H]: SetAtPath<T[K], R, V> }
            : T
      : T;

/**
 * Sets the value at the given path in the object graph. If the path is empty, the value is returned.
 *
 * @param objectGraph the object graph to set the value.
 * @param path where to set the value.
 * @param value the value to set.
 * @returns Cloned object graph with the value set at the given path.
 */
export function setValue<
    const TObject,
    const P extends Path<TObject>,
    const TValue,
>(
    objectGraph: TObject,
    path: readonly [...P],
    value: TValue,
): SetAtPath<TObject, P, TValue> {
    return setValueImpl(objectGraph, path, value, [], true);
}

/**
 * Same with `setValue`, but returns the original object graph. This is useful when you want to directly mutate the object graph.
 *
 * @param objectGraph the object graph to set the value.
 * @param path where to set the value.
 * @param value the value to set.
 * @returns The original object graph with the value set at the given path without clone but strongly typed.
 */
export function setValueIn<
    const TObject,
    const P extends Path<TObject>,
    const TValue,
>(
    objectGraph: TObject,
    path: readonly [...P],
    value: TValue,
): SetAtPath<TObject, P, TValue> {
    return setValueImpl(objectGraph, path, value, [], false);
}

function setValueImpl<
    const TObject,
    const P extends Path<TObject>,
    const TValue,
>(
    objectGraph: TObject,
    path: readonly [...P],
    value: TValue,
    pathTraversed: (number | string)[],
    clone: boolean,
): SetAtPath<TObject, P, TValue> {
    if (path.length === 0) {
        // biome-ignore lint/suspicious/noExplicitAny: expected runtime code
        return value as any;
    }

    const [head, ...tail] = path;

    if (Array.isArray(objectGraph)) {
        const key = head as unknown as number;

        // biome-ignore lint/suspicious/noExplicitAny: expected runtime code
        const toEdit: any = clone ? objectGraph.slice() : objectGraph;

        if (!toEdit[key]) {
            throw new NoValueAtPathError(pathTraversed);
        }

        toEdit[key] = setValueImpl(
            toEdit[key],
            // biome-ignore lint/suspicious/noExplicitAny: expected runtime code
            tail as any,
            value,
            [...pathTraversed, key],
            clone,
        );
        return toEdit;
    }

    const key = head as unknown as keyof TObject;

    // biome-ignore lint/suspicious/noExplicitAny: expected runtime code
    const toEdit: any = clone ? { ...objectGraph } : objectGraph;

    if (!toEdit[key]) {
        throw new NoValueAtPathError(pathTraversed);
    }

    toEdit[key] = setValueImpl(
        toEdit[key],
        // biome-ignore lint/suspicious/noExplicitAny: expected runtime code
        tail as any,
        value,
        [...pathTraversed, key as unknown as number | string],
        clone,
    );
    return toEdit;
}

export class NoValueAtPathError extends Error {
    constructor(public readonly path: (number | string)[]) {
        super(`No value at path ${path.join(".")}`);
        super.name = this.constructor.name;
    }
}
