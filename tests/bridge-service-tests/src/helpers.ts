import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/**
 * Returns the host target triple reported by `rustc -vV`
 * (e.g. `x86_64-unknown-linux-gnu`).
 *
 * Throws if `rustc` is not on PATH or the output is unexpected.
 */
export async function getHost(): Promise<string> {
    let stdout: string;
    try {
        ({ stdout } = await execFileAsync("rustc", ["-vV"]));
    } catch (err) {
        throw new Error("Failed to run rustc to get the host target", {
            cause: err,
        });
    }

    const field = "host: ";
    const line = stdout.split("\n").find((l) => l.startsWith(field));
    if (!line) {
        throw new Error(
            `\`rustc -vV\` output had no "${field.trim()}" line:\n${stdout}`,
        );
    }
    return line.slice(field.length).trim();
}

export function delay(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

export const TEXT = {
    decode(data: Uint8Array) {
        return TEXT_DECODER.decode(data);
    },
    encode(str: string) {
        return TEXT_ENCODER.encode(str);
    },
};

export function json(unknown: unknown) {
    return TEXT_ENCODER.encode(JSON.stringify(unknown));
}
