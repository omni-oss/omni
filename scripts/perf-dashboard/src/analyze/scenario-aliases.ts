/**
 * Scenario alias map: canonicalizes scenario names that were renamed across
 * releases so version-history trends survive the rename. Sourced from a
 * checked-in constant merged with an env override (env wins). See DESIGN.md §6.2.
 */
import { type AliasMap, canonicalName, resolveAliasMap } from "./alias-map";

/** Maps historical scenario names onto a stable canonical key for trending. */
export type ScenarioAliasMap = AliasMap;

/** The reviewed baseline of known renames. Add entries here as presets evolve. */
export const CONSTANT_ALIASES: ScenarioAliasMap = {
    // e.g. "scale-300": "scale-300projects",
};

export const ALIAS_ENV_KEY = "PERF_DASHBOARD_SCENARIO_ALIASES";

/** Canonical name for a scenario under an alias map (identity when unmapped). */
export function canonicalScenario(
    name: string,
    aliases: ScenarioAliasMap,
): string {
    return canonicalName(name, aliases);
}

/**
 * Resolve the effective scenario alias map: the constant baseline merged with a
 * JSON override from {@link ALIAS_ENV_KEY} (env entries win). Invalid JSON is a
 * hard error rather than a silent no-op.
 */
export function resolveScenarioAliases(
    env: NodeJS.ProcessEnv = process.env,
): ScenarioAliasMap {
    return resolveAliasMap(ALIAS_ENV_KEY, CONSTANT_ALIASES, env);
}
