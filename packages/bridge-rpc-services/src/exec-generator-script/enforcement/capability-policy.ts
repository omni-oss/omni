import type { EnvRuleLayers } from "@omni-oss/bridge-rpc-system-interface";
import { globMatches } from "@omni-oss/bridge-rpc-system-interface";
import z from "zod";

// Re-exported so callers (and the spec) have one glob entry point regardless of
// which enforcement file they reach for; the implementation lives in the shared
// `@omni-oss/bridge-rpc-system-interface` module so every layer matches alike.
export { globMatches };

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
            processMatches(pattern, program),
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
    // Normalize both sides before matching so the same policy behaves
    // identically no matter which entry path a request arrives on: a `fetch`
    // host is already lowercased by the WHATWG URL parser, but a raw
    // `net.connect(port, "EVIL.COM")` / `Deno.connect` host is verbatim. Without
    // normalization a `deny evil.com` could be evaded with `EVIL.COM` or
    // `evil.com.` (trailing FQDN root). See {@link normalizeHost}.
    if (!globMatches(normalizeHost(pHost), normalizeHost(host))) {
        return false;
    }
    if (pPort === undefined || pPort === "*") {
        return true;
    }
    const parsed = Number.parseInt(pPort, 10);
    return Number.isInteger(parsed) && parsed === port;
}

/** Drop a single pair of surrounding `[` `]` from a (bracketed IPv6) host. */
function stripBrackets(host: string): string {
    return host.startsWith("[") && host.endsWith("]")
        ? host.slice(1, -1)
        : host;
}

/**
 * Canonicalize a host for case- and form-insensitive matching, reconciling every
 * shape a host reaches the matcher in:
 *
 * * **brackets** — a WHATWG `URL` hostname is bracketed (`[::1]`) while a raw
 *   `net.connect(port, "::1")` host is bare; strip them so a single grant covers
 *   both.
 * * **case** — DNS is case-insensitive, and `fetch` already lowercases via the
 *   URL parser; lowercasing here makes the raw-socket path agree so a `deny`
 *   cannot be dodged with `EVIL.COM`.
 * * **trailing dot** — `evil.com.` (the FQDN root) resolves the same as
 *   `evil.com`; strip it so it cannot dodge a rule either (matching the Rust
 *   `host_port_matches`, which strips the trailing dot).
 */
function normalizeHost(host: string): string {
    let h = stripBrackets(host).toLowerCase();
    if (h.length > 1 && h.endsWith(".")) {
        h = h.slice(0, -1);
    }
    return h;
}

/**
 * Split `host[:port]`, handling IPv6 so a colon inside the address is never
 * mistaken for the port delimiter (the twin of the Rust
 * `omni_capability_enforcement::lower::split_host_port`):
 *
 * * **Bracketed** (`[::1]`, `[::1]:443`) — the host is everything up to and
 *   including `]`; a port, if present, follows the closing bracket.
 * * **Bare** (`example.com:443`, `::1`) — the tail after the final colon is a
 *   port only when the host part has no further colon, so a bracket-less IPv6
 *   literal (`fe80::1`) is left intact rather than split on its own colons.
 */
function splitHostPort(pattern: string): {
    host: string;
    port: string | undefined;
} {
    if (pattern.startsWith("[")) {
        const close = pattern.indexOf("]");
        if (close === -1) {
            return { host: pattern, port: undefined }; // malformed — opaque host
        }
        const host = pattern.slice(0, close + 1);
        const after = pattern.slice(close + 1);
        if (after.startsWith(":")) {
            const p = after.slice(1);
            if (looksLikePort(p)) {
                return { host, port: p };
            }
        }
        return { host, port: undefined };
    }
    const idx = pattern.lastIndexOf(":");
    if (idx !== -1) {
        const p = pattern.slice(idx + 1);
        const head = pattern.slice(0, idx);
        if (looksLikePort(p) && !head.includes(":")) {
            return { host: head, port: p };
        }
    }
    return { host: pattern, port: undefined };
}

/** Whether `p` is a port selector: `*` (any) or all ASCII digits. */
function looksLikePort(p: string): boolean {
    return p === "*" || (p.length > 0 && /^[0-9]+$/.test(p));
}

/**
 * Whether the shim is running on Windows. The shim executes on the same host as
 * omni, so the local platform tells us how program paths are shaped (Windows
 * paths are `\`-separated, drive-lettered, and case-insensitive). We read the
 * runtime-appropriate signal because each runtime exposes it differently.
 */
const IS_WINDOWS: boolean = ((): boolean => {
    const g = globalThis as {
        Deno?: { build?: { os?: string } };
        process?: { platform?: string };
    };
    if (typeof g.Deno?.build?.os === "string") {
        return g.Deno.build.os === "windows";
    }
    return g.process?.platform === "win32";
})();

/**
 * Match a spawn `program` against a `process` pattern.
 *
 * The wrinkle is Windows: the *same* program reaches the matcher in different
 * shapes depending on which runtime resolved it — Deno hands over the bare name
 * (`cmd.exe`) while Node/Bun expand it to a full path (`C:\WINDOWS\system32\
 * cmd.exe` from `%ComSpec%`) — and Windows paths are case-insensitive. So one
 * policy pattern can authorize a program regardless of the runtime, we match the
 * full strings OR their basenames, case-folded. POSIX stays verbatim and
 * case-sensitive so a `/bin/sh` grant is not silently loosened to any `sh`. This
 * is the twin of the Rust `process_matches`. `isWindows` is injectable so the
 * platform-specific behaviour is testable off Windows; it defaults to the host.
 */
export function processMatches(
    pattern: string,
    program: string,
    isWindows: boolean = IS_WINDOWS,
): boolean {
    if (!isWindows) {
        return globMatches(pattern, program);
    }
    const pat = pattern.toLowerCase();
    const prog = program.toLowerCase();
    // Three fallbacks, because runtimes disagree on every axis: full path, then
    // basename (Deno passes bare `cmd.exe`, Node/Bun the full `…\cmd.exe`), then
    // basename with the executable extension stripped (Windows resolves a bare
    // `git` to `git.exe` via PATHEXT, so a `git` grant must reach `git.exe`).
    return (
        globMatches(pat, prog) ||
        globMatches(basename(pat), basename(prog)) ||
        globMatches(stripExeExt(basename(pat)), stripExeExt(basename(prog)))
    );
}

/** Final `/`- or `\`-separated component (Windows accepts both separators). */
function basename(s: string): string {
    const i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\"));
    return i === -1 ? s : s.slice(i + 1);
}

/**
 * Strip a trailing Windows executable extension (a subset of PATHEXT) so a bare
 * `git` grant matches a resolved `git.exe`. Input is already lowercased.
 */
function stripExeExt(s: string): string {
    for (const ext of [".exe", ".com", ".bat", ".cmd"]) {
        if (s.endsWith(ext)) {
            return s.slice(0, -ext.length);
        }
    }
    return s;
}
