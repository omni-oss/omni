export interface RunningTarget {
    apiBaseUrl: string;
    stop: () => Promise<void>;
}

export interface Target {
    name: string;
    start: (port: number) => Promise<RunningTarget>;
}
