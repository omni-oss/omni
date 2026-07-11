import type { Report } from "../chart/ir";
import type { Renderer, RenderOutput } from "./types";

/**
 * Passthrough renderer: emits the whole {@link Report} as pretty-printed JSON.
 * This is the contract/test oracle and the "generic data structure encoded with
 * all the chart types and data points" deliverable. See DESIGN.md §8.
 */
export interface JsonRendererOptions {
    /** Output file name. Defaults to "report.json". */
    fileName?: string;
    /** JSON indentation. Defaults to 2. */
    indent?: number;
}

export class JsonRenderer implements Renderer {
    readonly id = "json";

    constructor(private readonly options: JsonRendererOptions = {}) {}

    render(report: Report): Promise<RenderOutput> {
        const fileName = this.options.fileName ?? "report.json";
        const indent = this.options.indent ?? 2;
        return Promise.resolve({
            files: [
                {
                    path: fileName,
                    content: JSON.stringify(report, null, indent),
                    mime: "application/json",
                },
            ],
        });
    }
}
