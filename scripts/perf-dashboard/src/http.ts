/**
 * Shared HTTP retry helper: retries retryable status codes and network errors
 * with exponential backoff (honoring `Retry-After`). Used by the GitHub data
 * source and the AI analysis calls.
 */

/** Status codes worth retrying (transient: rate limits / temporary overload). */
export const RETRYABLE_STATUS = new Set([408, 429, 500, 502, 503, 504]);

export interface FetchRetryOptions {
    /** Max attempts (default 4). */
    maxAttempts?: number;
    /** Injectable fetch (for tests). Defaults to the global `fetch`. */
    fetchImpl?: typeof fetch;
    /** Base backoff in ms; doubles each attempt (default 2000). */
    baseDelayMs?: number;
    /** Optional hook for observability (attempt is the one that just failed). */
    onRetry?: (info: {
        attempt: number;
        status?: number;
        delayMs: number;
    }) => void;
}

/**
 * Fetch with exponential-backoff retry. Returns the final {@link Response}
 * (which may still be non-ok if all attempts returned a retryable status);
 * throws only when every attempt threw a network error.
 */
export async function fetchWithRetry(
    url: string,
    init: RequestInit,
    options: FetchRetryOptions = {},
): Promise<Response> {
    const fetchImpl = options.fetchImpl ?? globalThis.fetch;
    const maxAttempts = Math.max(1, options.maxAttempts ?? 4);
    const baseDelayMs = options.baseDelayMs ?? 2000;

    let lastError: unknown;
    for (let attempt = 1; attempt <= maxAttempts; attempt++) {
        let res: Response;
        try {
            res = await fetchImpl(url, init);
        } catch (err) {
            lastError = err;
            if (attempt === maxAttempts) throw err;
            const delayMs = baseDelayMs * 2 ** (attempt - 1);
            options.onRetry?.({ attempt, delayMs });
            await delay(delayMs);
            continue;
        }

        if (
            res.ok ||
            !RETRYABLE_STATUS.has(res.status) ||
            attempt === maxAttempts
        ) {
            return res;
        }

        // Honor Retry-After when present, else exponential backoff.
        const retryAfter = Number(res.headers.get("retry-after"));
        const delayMs =
            Number.isFinite(retryAfter) && retryAfter > 0
                ? retryAfter * 1000
                : baseDelayMs * 2 ** (attempt - 1);
        options.onRetry?.({ attempt, status: res.status, delayMs });
        await delay(delayMs);
    }

    // Unreachable in practice (loop returns/throws), but satisfies the compiler.
    throw lastError ?? new Error(`request to ${url} failed`);
}

function delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
