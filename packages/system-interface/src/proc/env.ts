import type { Env, ProcessEnv } from "./interfaces";

/**
 * The default {@link Env} implementation: a thin, read-only view over a raw
 * environment dictionary ({@link ProcessEnv}).
 *
 * It performs no filtering — every variable in the backing dictionary is
 * visible — so it is the right choice when the environment has *already* been
 * confined upstream (e.g. the Rust broker snapshot) or when no capability
 * policy applies. To additionally gate reads against an `env` capability
 * policy, use the capability-filtered env instead.
 */
export class ObjectEnv implements Env {
    constructor(private readonly vars: ProcessEnv) {}

    get(name: string): string | null {
        const value = this.vars[name];
        return value === undefined ? null : value;
    }

    toObject(): Record<string, string> {
        const out: Record<string, string> = {};
        for (const [key, value] of Object.entries(this.vars)) {
            if (value !== undefined) {
                out[key] = value;
            }
        }
        return out;
    }

    keys(): string[] {
        return Object.keys(this.vars);
    }
}
