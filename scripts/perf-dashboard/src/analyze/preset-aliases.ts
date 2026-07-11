/**
 * Preset alias map: canonicalizes preset names onto stable preset keys so any
 * code that checks preset names (notably the version-history minimum-data gate)
 * recognizes the free-form suite `name` that task-bench writes into `data.json`.
 *
 * task-bench's `suite -p <key>` resolves a built-in preset whose serialized
 * `SuiteResult.name` is the preset's descriptive `name` field (e.g. the `full`
 * preset serializes as "full suite"), not the CLI key. The baseline below maps
 * those built-in names back to their keys (mirrors
 * scripts/task-bench/src/suite/preset.ts). Sourced from a checked-in constant
 * merged with an env override (env wins), like scenario-aliases.
 */
import { type AliasMap, canonicalName, resolveAliasMap } from "./alias-map";

/** Maps a preset name (as it appears in data) onto a canonical preset key. */
export type PresetAliasMap = AliasMap;

/**
 * Built-in task-bench preset `name` → canonical CLI key. Keep in sync with
 * `BUILTIN_PRESETS` in scripts/task-bench/src/suite/preset.ts.
 */
export const CONSTANT_PRESET_ALIASES: PresetAliasMap = {
    "quick smoke suite": "quick",
    "dependency-shape sweep": "shapes",
    "scale sweep": "scale",
    "task-density sweep": "density",
    "daemon on vs off": "daemon",
    "full suite": "full",
};

export const PRESET_ALIAS_ENV_KEY = "PERF_DASHBOARD_PRESET_ALIASES";

/** Canonical preset key for a preset name (identity when unmapped). */
export function canonicalPreset(name: string, aliases: PresetAliasMap): string {
    return canonicalName(name, aliases);
}

/**
 * Resolve the effective preset alias map: the built-in baseline merged with a
 * JSON override from {@link PRESET_ALIAS_ENV_KEY} (env entries win). Invalid
 * JSON is a hard error rather than a silent no-op.
 */
export function resolvePresetAliases(
    env: NodeJS.ProcessEnv = process.env,
): PresetAliasMap {
    return resolveAliasMap(PRESET_ALIAS_ENV_KEY, CONSTANT_PRESET_ALIASES, env);
}
