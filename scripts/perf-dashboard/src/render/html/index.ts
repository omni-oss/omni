import type { Report } from "../../chart/ir";
import type { Renderer, RenderOutput } from "../types";
import CLIENT_JS from "./assets/client.js?raw";
import STYLES from "./assets/styles.css?raw";
import TEMPLATE from "./template.html?raw";

/**
 * Renders a whole {@link Report} to an interactive HTML page: the report JSON
 * is embedded and hydrated by inline vanilla JS into tabs (one per view), each
 * with interactive charts (Apache ECharts), data tables, a mandatory exclusion
 * callout, and provenance "view source" links.
 *
 * Charts use ECharts loaded from a CDN (configurable via {@link
 * HtmlRendererOptions.echartsUrl}) for responsive sizing and axis zoom/scroll,
 * so dense data stays legible. If the CDN is unavailable the data tables remain
 * as a full fallback. The page is assembled from raw-imported assets in this
 * folder: `template.html`, `assets/styles.css`, and the typed
 * `assets/client.js`. See DESIGN.md §8.
 */
export interface HtmlRendererOptions {
    fileName?: string;
    /** URL of the ECharts UMD bundle to load. Defaults to a pinned jsDelivr CDN. */
    echartsUrl?: string;
    /** URL of the marked UMD bundle to load. Defaults to a pinned jsDelivr CDN. */
    markedUrl?: string;
}

const DEFAULT_ECHARTS_URL =
    "https://cdn.jsdelivr.net/npm/echarts@5.5.1/dist/echarts.min.js";

const DEFAULT_MARKED_URL =
    "https://cdn.jsdelivr.net/npm/marked@15/marked.min.js";

export class HtmlRenderer implements Renderer {
    readonly id = "html";

    constructor(private readonly options: HtmlRendererOptions = {}) {}

    render(report: Report): Promise<RenderOutput> {
        return Promise.resolve({
            files: [
                {
                    path: this.options.fileName ?? "index.html",
                    content: renderHtml(
                        report,
                        this.options.echartsUrl ?? DEFAULT_ECHARTS_URL,
                        this.options.markedUrl ?? DEFAULT_MARKED_URL,
                    ),
                    mime: "text/html",
                },
            ],
        });
    }
}

function renderHtml(
    report: Report,
    echartsUrl: string,
    markedUrl: string,
): string {
    // Embed as JSON in a raw-text <script>; escape "<" so it can't break out.
    const data = JSON.stringify(report).replace(/</g, "\\u003c");
    // Function replacements so "$" in asset/data content is never interpreted.
    return TEMPLATE.replace(/\{\{TITLE\}\}/g, () => escapeHtml(report.title))
        .replace(/\{\{STYLES\}\}/g, () => STYLES)
        .replace(/\{\{MARKED_SRC\}\}/g, () => escapeHtml(markedUrl))
        .replace(/\{\{ECHARTS_SRC\}\}/g, () => escapeHtml(echartsUrl))
        .replace(/\{\{DATA\}\}/g, () => data)
        .replace(/\{\{CLIENT\}\}/g, () => CLIENT_JS);
}

function escapeHtml(s: string): string {
    return s.replace(
        /[&<>"]/g,
        (c) =>
            ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c,
    );
}
