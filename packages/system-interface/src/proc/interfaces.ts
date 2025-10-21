export interface Process {
    currentDir(): string;
    setCurrentDir(dir: string): void;
    args(): string[];
    env(): ProcessEnv;
}

export interface ProcessEnv {
    [key: string]: string | undefined;
}
