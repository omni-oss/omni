import type { Report } from "../chart/ir";

/**
 * Renderer abstraction. A renderer consumes a whole {@link Report} — all views —
 * and emits a single output artifact. Renderers depend only on the Chart IR
 * (`../chart`), never on task-bench or the ingest layer. See DESIGN.md §8.
 */

export interface RenderFile {
    path: string;
    content: string | Uint8Array;
    mime: string;
}

export interface RenderOutput {
    files: RenderFile[];
}

export interface Renderer {
    /** Stable id, e.g. "json" | "markdown" | "html". */
    readonly id: string;
    /** Render a whole Report — all views — into a single output artifact. */
    render(report: Report): Promise<RenderOutput>;
}
