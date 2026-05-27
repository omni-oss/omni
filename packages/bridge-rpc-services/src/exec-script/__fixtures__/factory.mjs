// Fixture for loader.spec.ts — factory function as default export.
// loadScript should NOT invoke this; it returns the function as-is.
/**
 * @param {any} opts
 */
export default function factory(opts) {
    return { kind: "factory", opts };
}
