import type { EnvRuleLayers } from "@omni-oss/bridge-rpc-system-interface";
import z from "zod";

/**
 * The residual capability policy the runtime shim must enforce, mirroring the
 * Rust `omni_capability_enforcement::ShimPolicy` wire format. It is **layered**
 * so the shim can apply the same shrink-only (attenuation) fold the Rust broker
 * uses for `fs`/`env`:
 *
 * * `enforced` names the domains the shim is responsible for (`net`, `process`)
 *   — the ones the runtime's launch flags could not confine precisely. A domain
 *   absent here is left to the runtime (pure passthrough). A domain present here
 *   but granted by no layer is deny-all (the fold's fail-closed rule).
 * * `layers` carries each policy level's rules, ordered outermost → innermost
 *   (workspace floor, ancestor generators, this generator, this action). Each
 *   layer maps a domain to its `allow` / `deny` pattern lists in the policy's
 *   neutral vocabulary (`host:port` for `net`, a program name/glob for
 *   `process`). A layer omits a domain it does not constrain (pass-through for
 *   that level).
 */
const DomainRulesSchema = z.object({
    allow: z.array(z.string()).default([]),
    deny: z.array(z.string()).default([]),
});

const ShimLayerSchema = z.record(z.string(), DomainRulesSchema);

const ShimPolicySchema = z.object({
    enforced: z.array(z.string()).default([]),
    layers: z.array(ShimLayerSchema).default([]),
});

export type DomainRules = z.infer<typeof DomainRulesSchema>;
type ShimLayer = Map<string, DomainRules>;

const NET = "net";
const PROCESS = "process";
const ENV = "env";

/**
 * A parsed, queryable capability policy. Evaluation folds the layers with the
 * **shrink-only (attenuation) model**, the exact TypeScript twin of the Rust
 * `omni_capabilities::evaluate_layered`: iterating outermost → innermost, a
 * request is allowed **iff** (1) no level explicitly denies it, (2) no level
 * that whitelists the domain blocks it (a deeper level can never reach outside
 * an upstream level's allow-list — the attenuation / ceiling rule), and (3) at
 * least one level actively grants it (fail-closed). Adding a level can only keep
 * the verdict or turn `allow` into `deny`; it can never widen authority.
 *
 * A single-layer policy reduces to the old deny-dominant, fail-closed decision,
 * so behaviour is unchanged for a lone generator.
 */
export class CapabilityPolicy {
    private constructor(
        private readonly enforced: Set<string>,
        private readonly layers: ShimLayer[],
    ) {}

    /** An empty policy: the shim enforces nothing (pure passthrough). */
    static empty(): CapabilityPolicy {
        return new CapabilityPolicy(new Set(), []);
    }

    /**
     * Parse the JSON residual passed via `--enforce`. A missing/blank/invalid
     * value yields an {@link empty} policy rather than throwing, so a
     * mis-passed flag degrades to "the runtime flags are the only enforcement"
     * rather than crashing the bridge — the fail-closed floor still applies at
     * the runtime/OS layer.
     */
    static parse(json: string | null | undefined): CapabilityPolicy {
        if (!json || json.trim() === "") {
            return CapabilityPolicy.empty();
        }
        let raw: unknown;
        try {
            raw = JSON.parse(json);
        } catch {
            return CapabilityPolicy.empty();
        }
        const parsed = ShimPolicySchema.safeParse(raw);
        if (!parsed.success) {
            return CapabilityPolicy.empty();
        }
        const layers = parsed.data.layers.map(
            (layer) => new Map(Object.entries(layer)),
        );
        return new CapabilityPolicy(new Set(parsed.data.enforced), layers);
    }

    /** Whether the shim is responsible for the `net` domain. */
    hasNet(): boolean {
        return this.enforced.has(NET);
    }

    /** Whether the shim is responsible for the `process` domain. */
    hasProcess(): boolean {
        return this.enforced.has(PROCESS);
    }

    /** Whether the shim is responsible for the `env` domain. */
    hasEnv(): boolean {
        return this.enforced.has(ENV);
    }

    /**
     * Whether a connection to `host:port` is permitted by the `net` policy.
     * Returns `true` when the shim does not enforce `net` (the runtime does).
     */
    checkNet(host: string, port: number): boolean {
        if (!this.enforced.has(NET)) {
            return true;
        }
        return this.evaluate(NET, (pattern) => netMatches(pattern, host, port));
    }

    /**
     * Whether spawning `program` is permitted by the `process` policy. Returns
     * `true` when the shim does not enforce `process` (the runtime does).
     */
    checkProcess(program: string): boolean {
        if (!this.enforced.has(PROCESS)) {
            return true;
        }
        return this.evaluate(PROCESS, (pattern) =>
            globMatches(pattern, program),
        );
    }

    /**
     * Whether reading the environment variable `name` is permitted by the `env`
     * policy. Returns `true` when the shim does not enforce `env` (the runtime /
     * broker does), matching {@link checkNet} / {@link checkProcess}.
     */
    checkEnv(name: string): boolean {
        if (!this.enforced.has(ENV)) {
            return true;
        }
        return this.evaluate(ENV, (pattern) => globMatches(pattern, name));
    }

    /**
     * The layered `env` rules in the neutral `{ allow, deny }` shape consumed by
     * the `@omni-oss/system-interface` capability-filtered env, or `undefined`
     * when the shim does not enforce `env` (so `proc.env()` passes the
     * already-broker-filtered snapshot through verbatim). Handed to
     * `BridgeRpcSystem.create` so the RPC `proc.env()` view is filtered by the
     * very same rules the shim enforces.
     */
    envRuleLayers(): EnvRuleLayers | undefined {
        if (!this.enforced.has(ENV)) {
            return undefined;
        }
        return this.layers.map((layer) => {
            const rules = layer.get(ENV);
            return {
                allow: rules?.allow ?? [],
                deny: rules?.deny ?? [],
            };
        });
    }

    /**
     * Fold the layers for `domain` under the shrink-only model — the twin of the
     * Rust `evaluate_layered`. A layer that omits the domain is pass-through
     * (`Permit`); a matching `deny` at any layer is dominant; a layer with
     * `allow` rules none of which match blocks (the ceiling); the request is
     * allowed only if at least one layer granted it and none blocked it.
     */
    private evaluate(
        domain: string,
        matches: (pattern: string) => boolean,
    ): boolean {
        let granted = false;
        let blocked = false;
        for (const layer of this.layers) {
            const rules = layer.get(domain);
            if (!rules) {
                continue; // Permit: this level does not constrain the domain.
            }
            if (rules.deny.some(matches)) {
                return false; // Explicit deny is dominant.
            }
            if (rules.allow.some(matches)) {
                granted = true; // Grant.
            } else if (rules.allow.length > 0) {
                blocked = true; // Whitelist present but unmatched → ceiling.
            }
            // else: only non-matching deny rules → Permit (no effect).
        }
        return granted && !blocked;
    }
}

/**
 * Match a `host[:port]` pattern against a concrete `host` + `port`. The host
 * part is a glob (`*` / `?`), the port is exact, `*` (any), or omitted (any) —
 * matching the Rust `host_port_matches`.
 */
export function netMatches(
    pattern: string,
    host: string,
    port: number,
): boolean {
    const { host: pHost, port: pPort } = splitHostPort(pattern);
    if (!globMatches(pHost, host)) {
        return false;
    }
    if (pPort === undefined || pPort === "*") {
        return true;
    }
    const parsed = Number.parseInt(pPort, 10);
    return Number.isInteger(parsed) && parsed === port;
}

/**
 * Split `host[:port]`, recognising the port only when it is `*` or all-digits,
 * so bare hosts (and IPv6-ish patterns) are not mis-split. Mirrors the Rust
 * `split_host_port`.
 */
function splitHostPort(pattern: string): {
    host: string;
    port: string | undefined;
} {
    const idx = pattern.lastIndexOf(":");
    if (idx !== -1) {
        const p = pattern.slice(idx + 1);
        const looksLikePort = p === "*" || (p.length > 0 && /^[0-9]+$/.test(p));
        if (looksLikePort) {
            return { host: pattern.slice(0, idx), port: p };
        }
    }
    return { host: pattern, port: undefined };
}

/**
 * Glob match with `*` (any run of characters, including separators) and `?`
 * (a single character). All other characters are matched literally. This mirrors
 * the non-separator-aware glob the Rust backend uses for hosts and program
 * names.
 */
export function globMatches(pattern: string, value: string): boolean {
    return globToRegExp(pattern).test(value);
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
