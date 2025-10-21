type NodeFsPromises = typeof import("node:fs/promises");

let NodeFs: NodeFsPromises;

export async function loadNodeFs(): Promise<NodeFsPromises> {
    if (typeof NodeFs !== "undefined") {
        return NodeFs;
    }

    try {
        NodeFs = await import("node:fs/promises");
        return NodeFs;
    } catch (e) {
        throw new Error(
            "Node FS is not available, please use other file system implementation",
            {
                cause: e,
            },
        );
    }
}
