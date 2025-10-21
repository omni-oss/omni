export interface Process {
    currentDir(): string;
    setCurrentDir(dir: string): void;
}
