/** Right-pad a string to a fixed width. */
export function pad(value: string, width: number): string {
    return value.length >= width
        ? value
        : value + " ".repeat(width - value.length);
}

/**
 * Render a Markdown table (header + separator + rows) as an array of lines,
 * with each column auto-sized to its widest cell.
 */
export function renderTable(headers: string[], rows: string[][]): string[] {
    const widths = headers.map((h, i) =>
        Math.max(h.length, ...rows.map((r) => (r[i] ?? "").length)),
    );
    const line = (cells: string[]) =>
        `| ${cells.map((c, i) => pad(c, widths[i] ?? 0)).join(" | ")} |`;
    return [
        line(headers),
        `| ${widths.map((w) => "-".repeat(w)).join(" | ")} |`,
        ...rows.map(line),
    ];
}

/** Format a `prefix: tool ver, tool ver` version summary line. */
export function renderVersionList(
    pairs: Array<readonly [string, string | null | undefined]>,
    prefix: string,
): string[] {
    const parts = pairs.map(([tool, version]) => `${tool} ${version ?? "?"}`);
    return [`${prefix}: ${parts.join(", ")}`];
}
