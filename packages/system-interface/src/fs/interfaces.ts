export interface FileSystem {
    /**
     * Reads the entire contents of a file as a string.
     * @param path The path to the file.
     */
    readFileAsString(path: string): Promise<string>;

    /**
     * Reads the entire contents of a file as a bytearray.
     * @param path The path to the file.
     */
    readFileAsBytes(path: string): Promise<Uint8Array>;

    /**
     * Writes a string to a file, overwriting the file if it already exists.
     * @param path The path to the file.
     * @param content The string content to write.
     */
    writeStringToFile(path: string, content: string): Promise<void>;

    /**
     * Writes bytearray to a file, overwriting the file if it already exists.
     * @param path The path to the file.
     * @param content The string content to write.
     */
    writeBytesToFile(path: string, content: Uint8Array): Promise<void>;

    /**
     * Checks if the given path exists (file or directory).
     * @param path The path to check.
     */
    pathExists(path: string): Promise<boolean>;

    /**
     * Creates a new directory.
     * @param path The path of the directory to create.
     * @param options Options, typically including a recursive flag.
     */
    createDirectory(
        path: string,
        options?: { recursive?: boolean },
    ): Promise<void>;

    /**
     * Reads the contents of a directory.
     * @param path The path to the directory.
     * @returns A promise that resolves with an array of file/directory names in the directory.
     */
    readDirectory(path: string): Promise<string[]>;

    // --- Deletion and Movement ---

    /**
     * Removes a file or directory.
     * @param path The path to the file or directory to remove.
     * @param options Options, typically including a recursive flag for directories.
     */
    remove(path: string, options?: { recursive?: boolean }): Promise<void>;

    /**
     * Renames or moves a file or directory.
     * @param oldPath The original path.
     * @param newPath The new path.
     */
    rename(oldPath: string, newPath: string): Promise<void>;

    // --- Stat and Type Checking (Useful for 'pathExists' refinement) ---

    /**
     * Gets the stats (metadata) for the given path.
     * (Implementations can decide on a common Stat type or use an abstract one).
     * @param path The path to stat.
     * @returns A promise resolving to an object containing file metadata.
     */
    stat(path: string): Promise<FileStat>;

    /**
     * Checks if the given path is a file.
     * @param path The path to check.
     */
    isFile(path: string): Promise<boolean>;

    /**
     * Checks if the given path is a directory.
     * @param path The path to check.
     */
    isDirectory(path: string): Promise<boolean>;

    /**
     * Checks if the given path is a symbolic link.
     * @param path The path to check.
     */
    isSymbolicLink(path: string): Promise<boolean>;

    /**
     * Copies a file or directory.
     * @param src
     * @param dest
     * @param options
     */
    copy(
        src: string,
        dest: string,
        options?: { overwrite?: boolean; recursive?: boolean },
    ): Promise<void>;

    /**
     * Appends content to a file.
     * @param path
     * @param content
     */
    appendStringToFile(path: string, content: string): Promise<void>;
}

// A simplified abstract interface for file statistics
export interface FileStat {
    /** Whether the path refers to a file. */
    isFile(): boolean;
    /** Whether the path refers to a directory. */
    isDirectory(): boolean;
    /** Whether the path refers to a symbolic link. */
    isSymbolicLink(): boolean;
    /** Size of the file in bytes. */
    size: number;
    /** The last time the file was modified. */
    mtime: Date;
    // Add other properties like 'birthtime' or 'mode' as needed
}
