/**
 * The single glob implementation shared by every TypeScript enforcement point
 * (the `env` snapshot filter here, and the runtime shim's `net`/`process`
 * matchers in `@omni-oss/bridge-rpc-services`). Keeping one copy — rather than
 * the three that had drifted apart — guarantees the layers can never disagree
 * on what a pattern matches.
 *
 * `*` matches any run of characters (including none) and `?` matches exactly
 * one; every other character is literal. There is deliberately **no** path
 * separator awareness — the values matched (env names, hosts, program names)
 * are opaque strings, not paths — so this mirrors the Rust `glob_str_matches`
 * (globset with `literal_separator: false`).
 */
export function globMatches(pattern: string, value: string): boolean {
    return globToRegExp(pattern).test(value);
}

/** Compile a glob (`*` / `?`) to an anchored {@link RegExp}. */
export function globToRegExp(glob: string): RegExp {
    let out = "^";
    for (const ch of glob) {
        if (ch === "*") {
            out += ".*";
        } else if (ch === "?") {
            out += ".";
        } else {
            out += escapeRegExp(ch);
        }
    }
    out += "$";
    return new RegExp(out);
}

/** Escape a single character for literal use inside a {@link RegExp}. */
export function escapeRegExp(ch: string): string {
    return ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
