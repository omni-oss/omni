import type { PathLike } from "node:fs";

type NodeFsPromises = Omit<typeof import("node:fs/promises"), "exists"> & {
    exists: (path: PathLike) => Promise<boolean>;
};

let NodeFsPromises: NodeFsPromises;

export async function loadNodeFs(): Promise<NodeFsPromises> {
    if (typeof NodeFsPromises !== "undefined") {
        return NodeFsPromises;
    }

    try {
        const fsAsync = {
            ...((await import("node:fs/promises")) as NodeFsPromises),
        };
        const fsSync = await import("node:fs");

        if (typeof fsAsync.exists === "undefined") {
            fsAsync.exists = async (path: PathLike): Promise<boolean> => {
                return promisifyNoErr(fsSync.exists)(path);
            };
        }

        NodeFsPromises = fsAsync;
        return NodeFsPromises;
    } catch (e) {
        throw new Error(
            "Node FS is not available, please use other file system implementation",
            {
                cause: e,
            },
        );
    }
}

// biome-ignore lint/suspicious/noExplicitAny: generic
export function promisify<TArgs extends any[], TError, TResult>(
    fn: (...args: [...TArgs, (error: TError, res?: TResult) => void]) => void,
) {
    return (...args: TArgs): Promise<TResult> => {
        return new Promise((resolve, reject) => {
            fn(...args, (err, result) => {
                if (err) reject(err);
                else resolve(result as TResult);
            });
        });
    };
}

// biome-ignore lint/suspicious/noExplicitAny: generic
export function promisifyNoErr<TArgs extends any[], TResult>(
    fn: (...args: [...TArgs, (res: TResult) => void]) => void,
) {
    return (...args: TArgs): Promise<TResult> => {
        return new Promise((resolve, _reject) => {
            fn(...args, (result) => {
                return resolve(result as TResult);
            });
        });
    };
}
