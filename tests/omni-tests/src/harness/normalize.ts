/**
 * Output normalization helpers for portable string assertions.
 *
 * The `omni` binary (and the tasks it spawns) can emit platform-specific line
 * endings and trailing whitespace. Normalize captured output before comparing
 * it so assertions stay stable across Linux/macOS/Windows and across shells.
 */

/**
 * Collapse `\r\n` / lone `\r` into `\n` and strip trailing newlines.
 *
 * Leading whitespace and interior blank lines are preserved so that
 * indentation-sensitive output (help text, schemas) still compares correctly.
 */
export function normalize(text: string): string {
    return text.replace(/\r\n?/g, "\n").replace(/\n+$/, "");
}

/**
 * Like {@link normalize} but also trims trailing whitespace on every line.
 *
 * Useful when a task or formatter pads lines with spaces that you don't want
 * to assert on.
 */
export function normalizeLines(text: string): string {
    return normalize(text)
        .split("\n")
        .map((line) => line.replace(/[ \t]+$/, ""))
        .join("\n");
}

/**
 * Split normalized text into an array of lines, dropping a trailing empty line.
 *
 * Handy for asserting on line-oriented output such as `omni project list`.
 */
export function lines(text: string): string[] {
    const normalized = normalize(text);
    return normalized.length === 0 ? [] : normalized.split("\n");
}
