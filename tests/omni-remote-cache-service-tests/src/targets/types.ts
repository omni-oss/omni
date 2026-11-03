export interface RunningTarget {
    apiBaseUrl: string;
    stop: () => Promise<void>;
}

export interface Target {
    name: string;
    /** Targets use disjoint port ranges so tasks can run concurrently. */
    basePort: number;
    start: (port: number) => Promise<RunningTarget>;
}
