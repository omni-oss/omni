/**
 * Shared machinery for alias maps that canonicalize names across releases: a
 * checked-in constant baseline merged with a JSON object from an env key (env
 * entries win). Used by scenario-aliases and preset-aliases.
 */

/** Maps a historical/alias name onto a stable canonical key. */
export type AliasMap = Record<string, string>;

/** Canonical name under an alias map (identity when unmapped). */
export function canonicalName(name: string, aliases: AliasMap): string {
    return aliases[name] ?? name;
}

/**
 * Resolve the effective alias map: the constant baseline shallow-merged with a
 * JSON object parsed from `envKey` (env entries win). Missing/empty env ⇒ just
 * the constant; invalid JSON or shape is a hard error rather than a silent
 * no-op.
 */
export function resolveAliasMap(
    envKey: string,
    constant: AliasMap,
    env: NodeJS.ProcessEnv = process.env,
): AliasMap {
    const raw = env[envKey]?.trim();
    if (!raw) return { ...constant };

    let override: unknown;
    try {
        override = JSON.parse(raw);
    } catch (e) {
        throw new Error(`${envKey} is not valid JSON: ${(e as Error).message}`);
    }
    if (
        typeof override !== "object" ||
        override === null ||
        Array.isArray(override)
    ) {
        throw new Error(
            `${envKey} must be a JSON object of { alias: canonical } string pairs`,
        );
    }
    for (const [k, v] of Object.entries(override)) {
        if (typeof v !== "string") {
            throw new Error(
                `${envKey}["${k}"] must be a string, got ${typeof v}`,
            );
        }
    }

    return { ...constant, ...(override as AliasMap) };
}
