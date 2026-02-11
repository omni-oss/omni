import { execFile } from "node:child_process";
import { promisify } from "node:util";

export function withTimeout<Args extends unknown[], R>(
    f: (...args: Args) => Promise<R>,
    ms: number,
): (...args: Args) => Promise<R> {
    return async (...args: Args): Promise<R> => {
        // biome-ignore lint/style/noNonNullAssertion: false
        let timeoutId: ReturnType<typeof setTimeout> = undefined!;

        const timeoutPromise = new Promise<never>((_, reject) => {
            timeoutId = setTimeout(() => {
                reject(new TimeoutError(ms, f.name));
            }, ms);
        });

        try {
            // Promise.race ensures the timeout actually interrupts the flow
            return await Promise.race([f(...args), timeoutPromise]);
        } finally {
            if (timeoutId) {
                clearTimeout(timeoutId);
            }
        }
    };
}

class TimeoutError extends Error {
    constructor(
        public readonly timeout: number,
        public readonly originalFunctionName?: string,
        message?: string,
    ) {
        const text = [`Timeout of ${timeout}ms exceeded`] as string[];
        if (originalFunctionName) {
            text.push(`in ${originalFunctionName}`);
        }
        if (message) {
            text.push(`: ${message}`);
        }
        super(text.join(" "));
        this.name = TimeoutError.name;
    }
}

const execFileAsync = promisify(execFile);

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
            `\`rustc -vV\` didn't have a line for "${field.trim()}", got:\n${stdout}`,
        );
    }

    return line.slice(field.length);
}

export function sleep(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
