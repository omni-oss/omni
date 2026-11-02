export interface RunningTarget {
    apiBaseUrl: string;
    stop: () => Promise<void>;
}

export interface Target {
    name: string;
    /** When true the instance is started once per process and shared. */
    shared: boolean;
    start: (port: number) => Promise<RunningTarget>;
}
