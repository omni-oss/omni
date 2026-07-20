import type { Env, ProcessEnv } from "@omni-oss/system-interface";

/**
 * Allow / deny glob-pattern lists for the `env` domain at a single policy
 * level. Patterns are matched against a variable **name** with
 * {@link matchEnvGlob} (`*` / `?` wildcards) — never against a filesystem path,
 * so there is no separator awareness.
 */
export interface EnvDomainRules {
    readonly allow: readonly string[];
    readonly deny: readonly string[];
}

/**
 * An ordered stack of policy levels (outermost → innermost: workspace floor,
 * ancestor generators, this generator, this action). Evaluated with the same
 * shrink-only (attenuation) fold the Rust broker and the runtime shim use, so
 * a deeper level can only ever narrow — never widen — an ancestor's allow-list.
 */
export type EnvRuleLayers = readonly EnvDomainRules[];

/**
 * Match a glob `pattern` against a variable `name`.
 *
 * `*` matches any run of characters (including none) and `?` matches exactly
 * one; every other character is literal. There is deliberately **no** path
 * separator awareness — environment names are opaque strings, not paths — so
 * this mirrors the Rust `glob_str_matches` (globset with
 * `literal_separator: false`) and the runtime shim's `globMatches`, keeping the
 * three enforcement points in lock-step.
 */
export function matchEnvGlob(pattern: string, name: string): boolean {
    return globToRegExp(pattern).test(name);
}

function globToRegExp(glob: string): RegExp {
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

function escapeRegExp(ch: string): string {
    return ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Whether reading the variable `name` is permitted by the layered `env` policy.
 *
 * Folds the layers outermost → innermost under the shrink-only model — the twin
 * of the Rust `evaluate_layered` and the shim's `CapabilityPolicy.evaluate`: a
 * matching `deny` at any level is dominant; a level that whitelists names none
 * of which match blocks (the ceiling); the read is permitted only if at least
 * one level granted it and none blocked it (fail-closed). An empty stack denies
 * everything.
 */
export function envLayersAllow(layers: EnvRuleLayers, name: string): boolean {
    let granted = false;
    let blocked = false;
    for (const rules of layers) {
        if (rules.deny.some((p) => matchEnvGlob(p, name))) {
            return false; // Explicit deny is dominant.
        }
        if (rules.allow.some((p) => matchEnvGlob(p, name))) {
            granted = true; // Grant.
        } else if (rules.allow.length > 0) {
            blocked = true; // Whitelist present but unmatched → ceiling.
        }
        // else: only non-matching deny rules → no effect (pass-through).
    }
    return granted && !blocked;
}

/**
 * What {@link CapabilityFilteredEnv.get} does when the policy denies a read:
 *
 * * `throw` (the default) — raise an {@link EnvAccessDeniedError}, so an
 *   explicit read of a forbidden variable fails loudly rather than silently
 *   looking unset.
 * * `return-null` — behave like an unset variable and return `null`, so a
 *   script that probes an unknown/forbidden name degrades quietly.
 */
export type OnDeniedEnvAccess = "throw" | "return-null";

/**
 * Thrown by {@link CapabilityFilteredEnv.get} when the `env` policy denies a
 * read and the env was configured with `onDenied: "throw"`.
 */
export class EnvAccessDeniedError extends Error {
    constructor(public readonly variableName: string) {
        super(
            `environment variable "${variableName}" is not permitted by the capability policy`,
        );
        this.name = "EnvAccessDeniedError";
    }
}

/**
 * An {@link Env} that filters a raw environment dictionary against a layered
 * `env` capability policy: {@link get} throws (or returns `null`, see
 * {@link OnDeniedEnvAccess}) and {@link toObject} omits any variable whose name
 * the policy does not permit.
 *
 * This is the defense-in-depth twin of the Rust broker's snapshot filter — the
 * same env rules are handed to the runtime shim — so a script that reads the
 * environment through `ctx.sys.proc.env()` only ever observes the variables its
 * generator is allowed to see, regardless of what the underlying snapshot
 * carried.
 */
export class CapabilityFilteredEnv implements Env {
    constructor(
        private readonly vars: ProcessEnv,
        private readonly layers: EnvRuleLayers,
        private readonly onDenied: OnDeniedEnvAccess = "throw",
    ) {}

    /** Whether the policy permits reading `name`. */
    allows(name: string): boolean {
        return envLayersAllow(this.layers, name);
    }

    get(name: string): string | null {
        if (!this.allows(name)) {
            if (this.onDenied === "return-null") {
                return null;
            }
            throw new EnvAccessDeniedError(name);
        }
        const value = this.vars[name];
        return value === undefined ? null : value;
    }

    /**
     * A snapshot of only the permitted variables. This enumerates the allowed
     * set rather than reading a specific forbidden name, so it never throws —
     * denied variables are simply absent, regardless of {@link OnDeniedEnvAccess}.
     */
    toObject(): Record<string, string> {
        const out: Record<string, string> = {};
        for (const [key, value] of Object.entries(this.vars)) {
            if (value !== undefined && this.allows(key)) {
                out[key] = value;
            }
        }
        return out;
    }

    keys(): string[] {
        return Object.keys(this.vars).filter((key) => this.allows(key));
    }
}
