export interface RunningTarget {
    apiBaseUrl: string;
    /** Runs SQL against the target's database, when it has one. Returns JSON. */
    sql?: ((statement: string) => Promise<string>) | undefined;
    stop: () => Promise<void>;
}

export interface Target {
    name: string;
    /** Targets use disjoint port ranges so tasks can run concurrently. */
    basePort: number;
    start: (port: number) => Promise<RunningTarget>;
}
