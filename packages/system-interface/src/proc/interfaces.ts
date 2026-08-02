export interface Process {
    currentDir(): string;
    setCurrentDir(dir: string): Promise<void>;
    args(): ArgsList;
    env(): Env;
}

/**
 * A read-only view over a set of environment variables.
 *
 * Implementations may expose the process environment verbatim (see
 * {@link ObjectEnv}) or filter it against a capability policy so a script only
 * ever observes the variables it is permitted to read (see
 * `@omni-oss/system-interface`'s capability-filtered env). Consumers should
 * treat the result as an immutable snapshot.
 */
export interface Env {
    /**
     * The value of the variable `name`, or `null` when it is unset — or, for a
     * capability-filtered implementation, when the policy does not permit
     * reading it.
     */
    get(name: string): string | null;
    /**
     * A plain, mutation-safe key→value snapshot clone of every readable
     * variable. Undefined entries are omitted.
     */
    toObject(): Record<string, string>;

    keys(): string[];
}

/**
 * The raw environment dictionary shape: a plain key→value record where an
 * absent variable is `undefined`. This is the backing store an {@link Env}
 * implementation wraps, not the `Env` view itself.
 */
export interface ProcessEnv {
    readonly [key: string]: string | undefined;
}

export type ArgsList = readonly string[];
