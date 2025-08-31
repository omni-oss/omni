import { searchServer } from "@/lib/search-server.shared";

let isFetchPatched = false;

const windowFetch: typeof window.fetch | null =
    typeof window !== "undefined" && typeof window.fetch === "function"
        ? window.fetch
        : null;

export type PatchFetchConfig = {
    searchApiPath?: string;
};

export function patchFetch({
    searchApiPath = "/api/search",
}: PatchFetchConfig = {}) {
    if (isFetchPatched) return;
    if (windowFetch) {
        const patchedFetch = ((
            input: RequestInfo | URL,
            init?: RequestInit,
        ) => {
            if (
                (typeof input === "string" && input === searchApiPath) ||
                (input instanceof URL &&
                    input.origin === window.location.origin &&
                    input.pathname === searchApiPath)
            ) {
                return searchServer.staticGET();
            }

            return windowFetch(input, init);
        }) as typeof window.fetch;

        window.fetch = patchedFetch;
        isFetchPatched = true;
        console.log("fetch patched");
    }
}
