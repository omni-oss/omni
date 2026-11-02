import { sleep, withTimeout } from "../utils";

export interface WaitOptions {
    tries: number;
    delayMs: number;
    fetchTimeoutMs: number;
    /** Returns a reason if the process died, to fail fast. */
    exited: () => string | null;
    describe: () => string;
}

/** Waits until the server answers any HTTP response (status is irrelevant). */
export async function waitForServer(url: string, opts: WaitOptions) {
    const probe = withTimeout(fetch, opts.fetchTimeoutMs);
    for (let i = 0; i < opts.tries; i++) {
        try {
            await probe(url);
            return;
        } catch {
            // not up yet
        }
        const reason = opts.exited();
        if (reason) {
            throw new Error(`Server ${reason}:\n${opts.describe()}`);
        }
        await sleep(opts.delayMs);
    }
    throw new Error(`Failed to connect to ${url}\n${opts.describe()}`);
}
