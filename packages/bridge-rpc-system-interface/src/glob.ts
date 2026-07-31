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

/** Compile a glob (`*` / `?`) to a fully-anchored, dotAll {@link RegExp}. */
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
    // `(?![\s\S])` is a true end-of-string anchor: unlike `$`, it does not also
    // match just *before* a trailing "\n", so `example.com` can never match
    // `example.com\n`. The `s` (dotAll) flag lets `.` / `.*` span newlines, so a
    // value containing "\n" is still caught by `*` (including a deny-all `*`).
    // Together these mirror Rust `globset`, which is whole-string anchored and
    // treats newlines as ordinary characters.
    out += "(?![\\s\\S])";
    return new RegExp(out, "s");
}

/** Escape a single character for literal use inside a {@link RegExp}. */
export function escapeRegExp(ch: string): string {
    return ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
