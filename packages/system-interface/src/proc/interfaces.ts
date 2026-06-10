export interface Process {
    currentDir(): string;
    setCurrentDir(dir: string): Promise<void>;
    args(): ArgsList;
    env(): ProcessEnv;
}

export interface ProcessEnv {
    readonly [key: string]: string | undefined;
}

export type ArgsList = readonly string[];
