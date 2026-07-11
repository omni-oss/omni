import { HtmlRenderer } from "./html";
import { JsonRenderer } from "./json";
import { MarkdownRenderer } from "./markdown";
import type { Renderer } from "./types";

export { axisLabel, formatBytes, formatValue } from "../format";
export type { HtmlRendererOptions } from "./html";
export { HtmlRenderer } from "./html";
export type { JsonRendererOptions } from "./json";
export { JsonRenderer } from "./json";
export type { MarkdownRendererOptions } from "./markdown";
export { MarkdownRenderer } from "./markdown";
export type { Renderer, RenderFile, RenderOutput } from "./types";

export type RendererId = "json" | "markdown" | "html";

/** Construct a renderer by id (used by the CLI), with an optional file-name override. */
export function getRenderer(
    id: string,
    options: { fileName?: string } = {},
): Renderer {
    const fileOpt = options.fileName ? { fileName: options.fileName } : {};
    switch (id) {
        case "json":
            return new JsonRenderer(fileOpt);
        case "markdown":
            return new MarkdownRenderer(fileOpt);
        case "html":
            return new HtmlRenderer(fileOpt);
        default:
            throw new Error(
                `unknown renderer "${id}" (expected json | markdown | html)`,
            );
    }
}
