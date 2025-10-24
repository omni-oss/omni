export function throwIfBunNotAvailable(): void {
    if (typeof Bun === "undefined") {
        throw new Error(
            "Bun is not available, please use other implementation",
        );
    }
}

export function throwIfDenoNotAvailable(): void {
    if (typeof Deno === "undefined") {
        throw new Error(
            "Deno is not available, please use other implementation",
        );
    }
}
