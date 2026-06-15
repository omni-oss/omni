import os from "node:os";
export function cleanPath(path: string): string {
    if (os.platform() === "win32") {
        // remove UNC prefix if present
        if (path.startsWith("\\\\?\\")) {
            path = path.slice(4);
        }
    }

    return path;
}
