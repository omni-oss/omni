// @ts-check
/**
 * Client-side hydration for the perf-dashboard HTML report.
 *
 * Reads the embedded `Report` JSON from `#report-data` and builds tabs (one per
 * view), each with interactive charts (Apache ECharts), data tables, a
 * mandatory exclusion callout, and provenance "view source" links.
 *
 * Charts use ECharts (loaded from a CDN by the page) for responsive sizing and
 * `dataZoom` so dense data stays legible — you can scroll/zoom the axis instead
 * of squeezing every category into a fixed width. If ECharts fails to load
 * (e.g. offline), the data tables remain as a full fallback.
 *
 * This file is imported as raw text by `../index.ts` and injected verbatim into
 * a `<script>` tag, so it must be valid standalone browser JS. It is typed via
 * JSDoc (`@ts-check`) rather than TypeScript. It mirrors the Chart IR in
 * `src/chart/ir.ts` — keep the typedefs below in sync.
 */

/**
 * @typedef {"ms" | "s" | "bytes" | "cores" | "count" | "%"} Unit
 * @typedef {{ x: string | number, y: number | null, yError?: number }} Point
 * @typedef {{ key: string, label: string, emphasis?: boolean, points: Point[] }} Series
 * @typedef {{ label: string, unit?: Unit }} Axis
 * @typedef {{ id: string, kind: string, title: string, subtitle?: string, x: Axis, y: Axis, series: Series[], analysis?: string, aiAnalysis?: string, facets?: Array<{ dimension: string, value: string }> }} Chart
 * @typedef {{ label: string, failed: string[], reason: string }} ExcludedItem
 * @typedef {{ title: string, criteria: string, items: ExcludedItem[] }} ExclusionPanel
 * @typedef {{ version: string, commitSha?: string, sourceUrl?: string, generatedAt?: string }} Provenance
 * @typedef {{ title: string, columns: string[], rows: string[][], open?: boolean }} InfoGroup
 * @typedef {{ id: string, title: string, description?: string, notes?: string[], charts: Chart[], exclusionPanel?: ExclusionPanel, provenance?: Provenance[], info?: InfoGroup[] }} View
 * @typedef {{ title: string, generatedAt: string, notes?: string[], analysis?: string, aiAnalysis?: string, views: View[] }} ReportData
 */

(() => {
    /** Series colour palette; index 0 is reserved for the emphasized series. */
    const PALETTE = [
        "#4f46e5",
        "#0ea5e9",
        "#f59e0b",
        "#10b981",
        "#ef4444",
        "#a855f7",
        "#14b8a6",
    ];

    const EM_DASH = "\u2014";

    /** The ECharts global, if the CDN script loaded. Typed loosely (no types). */
    const ECHARTS = /** @type {any} */ (window).echarts;

    /** The marked library, if the CDN script loaded. */
    const MARKED = /** @type {any} */ (window).marked;

    const dataNode = document.getElementById("report-data");
    const app = document.getElementById("app");
    if (!dataNode || !app) return;

    /** @type {ReportData} */
    const DATA = JSON.parse(dataNode.textContent || "{}");

    /** Live chart instances, tagged with the panel (view index) they belong to. */
    /** @type {Array<{ inst: any, panel: number }>} */
    const charts = [];

    /**
     * HTML-escape an arbitrary value for safe interpolation into markup.
     * @param {unknown} value
     * @returns {string}
     */
    const esc = (value) =>
        String(value).replace(
            /[&<>"]/g,
            (c) =>
                ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[
                    c
                ] || c,
        );

    /**
     * Escape a string for use inside a CSS attribute selector.
     * @param {string} value
     * @returns {string}
     */
    const cssEsc = (value) => String(value).replace(/["\\]/g, "\\$&");

    /**
     * Render a Markdown string to HTML. When `marked` is available (CDN loaded),
     * uses `marked.parse()` for full block-level rendering (paragraphs, headings,
     * lists, code blocks, etc.) and adds `target="_blank"` to every link.
     * Falls back to a minimal inline renderer (code, bold, italic, links) when
     * the CDN is unavailable so analysis text is never lost.
     * @param {string} s
     * @returns {string}
     */
    const md = (s) => {
        if (MARKED?.parse) {
            return /** @type {string} */ (MARKED.parse(s)).replace(
                /<a href=/g,
                '<a target="_blank" rel="noreferrer" href=',
            );
        }
        // Minimal inline fallback (used when CDN is unavailable).
        return esc(s)
            .replace(/`([^`]+)`/g, "<code>$1</code>")
            .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
            .replace(/\*([^*]+)\*/g, "<em>$1</em>")
            .replace(
                /\[([^\]]+)\]\(([^)]+)\)/g,
                '<a href="$2" target="_blank" rel="noreferrer">$1</a>',
            )
            .replace(/\n/g, "<br>");
    };

    /**
     * Analysis block. AI analysis wins and renders as a labeled, collapsible
     * <details> (open for the report, collapsed for a graph); synthetic
     * analysis renders inline (short, not collapsible).
     * @param {string|undefined} analysis
     * @param {string|undefined} aiAnalysis
     * @param {boolean} open
     * @returns {string}
     */
    const analysisHtml = (analysis, aiAnalysis, open) => {
        if (aiAnalysis) {
            return `<details class="analysis ai"${open ? " open" : ""}><summary>AI analysis</summary><div>${md(aiAnalysis)}</div></details>`;
        }
        if (analysis) return `<div class="analysis">${md(analysis)}</div>`;
        return "";
    };

    /**
     * Format a metric value for its unit. Mirrors `src/format.ts` — this is the
     * browser copy (the client is injected as raw text and cannot import).
     * @param {number | null | undefined} v
     * @param {Unit=} u
     * @returns {string}
     */
    const fmt = (v, u) => {
        if (v === null || v === undefined) return EM_DASH;
        if (u === "ms") {
            return v >= 1000
                ? `${(v / 1000).toFixed(2)}s`
                : `${Math.round(v)}ms`;
        }
        if (u === "bytes") {
            if (v >= 1073741824) return `${(v / 1073741824).toFixed(2)}GB`;
            if (v >= 1048576) return `${(v / 1048576).toFixed(0)}MB`;
            if (v >= 1024) return `${(v / 1024).toFixed(0)}KB`;
            return `${Math.round(v)}B`;
        }
        if (u === "cores") return `${v.toFixed(2)}\u00d7`;
        if (u === "%") return `${v.toFixed(1)}%`;
        return Number.isInteger(v) ? String(v) : v.toFixed(2);
    };

    /**
     * Colour for a series: emphasized series get the accent, others cycle.
     * @param {number} i
     * @param {Series} s
     * @returns {string}
     */
    const color = (i, s) => {
        const idx = s.emphasis ? 0 : (i + 1) % PALETTE.length;
        return PALETTE[idx] ?? PALETTE[0] ?? "#4f46e5";
    };

    /**
     * Build an ECharts option object for a chart. Nulls become gaps (bars are
     * omitted, lines break). `dataZoom` is added so many categories can be
     * scrolled/zoomed rather than crammed into a fixed width. Returns `any`
     * because ECharts ships no types here.
     * @param {Chart} chart
     * @returns {any}
     */
    const buildOption = (chart) => {
        const unit = chart.y.unit;
        const isLine = chart.kind === "line";
        const cats = (chart.series[0] ? chart.series[0].points : []).map(
            (p) => p.x,
        );
        const yName = unit ? `${chart.y.label} (${unit})` : chart.y.label;

        /**
         * @param {unknown} v
         * @returns {string}
         */
        const fmtVal = (v) => fmt(typeof v === "number" ? v : null, unit);

        return /** @type {any} */ ({
            color: chart.series.map((s, i) => color(i, s)),
            tooltip: { trigger: "axis", valueFormatter: fmtVal },
            legend: { type: "scroll", top: 0 },
            grid: {
                left: 8,
                right: 16,
                top: 40,
                bottom: 60,
                containLabel: true,
            },
            xAxis: {
                type: "category",
                name: chart.x.label,
                nameLocation: "middle",
                nameGap: 34,
                data: cats,
                boundaryGap: !isLine,
                axisLabel: { hideOverlap: true },
            },
            yAxis: {
                type: "value",
                name: yName,
                axisLabel: { formatter: fmtVal },
            },
            // Interactive scaling: wheel/pinch to zoom always; a scroll slider
            // appears once there are enough categories to be worth paging.
            dataZoom:
                cats.length > 6
                    ? [
                          { type: "inside" },
                          { type: "slider", height: 16, bottom: 8 },
                      ]
                    : [{ type: "inside" }],
            series: chart.series.map((s) => ({
                name: s.label,
                type: isLine ? "line" : "bar",
                data: s.points.map((p) => p.y),
                connectNulls: false,
                emphasis: { focus: "series" },
                ...(isLine
                    ? {
                          symbolSize: 6,
                          lineStyle: { width: s.emphasis ? 3 : 2 },
                      }
                    : {}),
            })),
        });
    };

    /**
     * Collapsible data table backing a chart (also the offline fallback).
     * @param {Chart} chart
     * @returns {string}
     */
    const tableHtml = (chart) => {
        const cats = (chart.series[0] ? chart.series[0].points : []).map(
            (p) => p.x,
        );
        const head = chart.series
            .map((s) => `<th>${esc(s.label)}</th>`)
            .join("");
        const rows = cats
            .map((cat, i) => {
                const cells = chart.series
                    .map((s) => {
                        const p = s.points[i];
                        return `<td>${esc(fmt(p ? p.y : null, chart.y.unit))}</td>`;
                    })
                    .join("");
                return `<tr><td>${esc(cat)}</td>${cells}</tr>`;
            })
            .join("");
        return `<details><summary>Data table</summary><table><thead><tr><th>${esc(chart.x.label)}</th>${head}</tr></thead><tbody>${rows}</tbody></table></details>`;
    };

    /**
     * Mandatory exclusion callout (versions dropped for insufficient data).
     * @param {ExclusionPanel} panel
     * @returns {string}
     */
    const exclusionHtml = (panel) => {
        const rows = panel.items
            .map((it) => {
                const badges =
                    it.failed
                        .map((f) => `<span class="badge">${esc(f)}</span>`)
                        .join("") || EM_DASH;
                return `<tr><td>${esc(it.label)}</td><td>${badges}</td><td>${esc(it.reason)}</td></tr>`;
            })
            .join("");
        return `<div class="callout"><h4>${esc(panel.title)}</h4><p class="muted">${esc(panel.criteria)}</p><table><thead><tr><th>version</th><th>missing</th><th>reason</th></tr></thead><tbody>${rows}</tbody></table></div>`;
    };

    /**
     * Per-version build provenance, with "view source" links.
     * @param {Provenance[]} prov
     * @returns {string}
     */
    const provenanceHtml = (prov) => {
        const rows = prov
            .map((p) => {
                const sha = p.commitSha ? p.commitSha.slice(0, 7) : EM_DASH;
                const build = p.sourceUrl
                    ? `<a href="${esc(p.sourceUrl)}" target="_blank" rel="noreferrer">${esc(sha)}</a>`
                    : esc(sha);
                return `<tr><td>${esc(p.version)}</td><td>${build}</td><td>${esc(p.generatedAt || EM_DASH)}</td></tr>`;
            })
            .join("");
        return `<details open><summary>Provenance</summary><table><thead><tr><th>version</th><th>build</th><th>generated</th></tr></thead><tbody>${rows}</tbody></table></details>`;
    };

    /**
     * A collapsible info panel (tool info / platform specs), rendered like the
     * per-chart data table.
     * @param {{ title: string, columns: string[], rows: string[][], open?: boolean }} group
     * @returns {string}
     */
    const infoHtml = (group) => {
        const head = group.columns.map((c) => `<th>${esc(c)}</th>`).join("");
        const body = group.rows
            .map(
                (row) =>
                    `<tr>${row.map((cell) => `<td>${esc(cell)}</td>`).join("")}</tr>`,
            )
            .join("");
        return `<details${group.open ? " open" : ""}><summary>${esc(group.title)}</summary><table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></details>`;
    };

    /**
     * Markup for a single view (chart canvases are hydrated afterwards). For
     * each facet dimension (e.g. target, metric, warmth) that has more than one
     * distinct value across the view's charts, a filter dropdown is added
     * ("All" + each value); a dimension with a single value gets no dropdown.
     * @param {View} view
     * @returns {string}
     */
    const viewHtml = (view) => {
        let h = "";
        if (view.description)
            h += `<p class="muted">${esc(view.description)}</p>`;
        if (view.notes?.length) {
            h += `<ul>${view.notes.map((n) => `<li>${esc(n)}</li>`).join("")}</ul>`;
        }
        if (view.exclusionPanel) h += exclusionHtml(view.exclusionPanel);
        if (view.provenance?.length) {
            h += provenanceHtml(view.provenance);
        }
        for (const group of view.info ?? []) {
            h += infoHtml(group);
        }

        // Collect facet dimensions (first-seen order) and their distinct values.
        /** @type {string[]} */
        const dims = [];
        /** @type {Record<string, string[]>} */
        const dimValues = {};
        for (const c of view.charts) {
            for (const f of c.facets ?? []) {
                if (!dims.includes(f.dimension)) {
                    dims.push(f.dimension);
                    dimValues[f.dimension] = [];
                }
                const arr = dimValues[f.dimension];
                if (arr && !arr.includes(f.value)) arr.push(f.value);
            }
        }
        // One dropdown per dimension with >1 value.
        let controls = "";
        for (const dim of dims) {
            const values = dimValues[dim] ?? [];
            if (values.length <= 1) continue;
            const label = dim.charAt(0).toUpperCase() + dim.slice(1);
            const opts = ['<option value="__all__">All</option>']
                .concat(
                    values.map(
                        (v) => `<option value="${esc(v)}">${esc(v)}</option>`,
                    ),
                )
                .join("");
            controls += `<label>${esc(label)}: <select class="facet-select" data-dim="${esc(dim)}">${opts}</select></label>`;
        }
        if (controls) h += `<div class="facet">${controls}</div>`;

        view.charts.forEach((c, ci) => {
            const attrs = (c.facets ?? [])
                .map((f) => ` data-facet-${f.dimension}="${esc(f.value)}"`)
                .join("");
            const analysis = analysisHtml(c.analysis, c.aiAnalysis, false);
            h += `<div class="chart"${attrs}><h3>${esc(c.title)}</h3>${analysis}<div class="echart" data-view="${esc(view.id)}" data-ci="${ci}"></div>${tableHtml(c)}</div>`;
        });
        if (view.charts.length === 0 && !view.exclusionPanel) {
            h += `<p class="muted">No charts.</p>`;
        }
        return h;
    };

    // --- assemble the page ------------------------------------------------

    let head = `<h1>${esc(DATA.title)}</h1><p class="muted">Generated ${esc(DATA.generatedAt)}</p>`;
    head += analysisHtml(DATA.analysis, DATA.aiAnalysis, true);
    if (DATA.notes?.length) {
        head += `<div class="callout">${DATA.notes.map(esc).join("<br>")}</div>`;
    }

    let tabs = "";
    let panels = "";
    DATA.views.forEach((view, i) => {
        const active = i === 0 ? " active" : "";
        tabs += `<button class="tab${active}" data-i="${i}">${esc(view.title)}</button>`;
        panels += `<section class="panel${active}" data-i="${i}">${viewHtml(view)}</section>`;
    });
    app.innerHTML =
        head +
        (DATA.views.length ? `<div class="tabs">${tabs}</div>` : "") +
        panels;

    // Hydrate charts. If ECharts didn't load, the data tables remain as the
    // fallback and we simply skip chart rendering.
    if (ECHARTS) {
        DATA.views.forEach((view, i) => {
            view.charts.forEach((chart, ci) => {
                const host = app.querySelector(
                    `.echart[data-view="${cssEsc(view.id)}"][data-ci="${ci}"]`,
                );
                if (host instanceof HTMLElement) {
                    const inst = ECHARTS.init(host);
                    inst.setOption(buildOption(chart));
                    charts.push({ inst, panel: i });
                }
            });
        });

        // Charts in hidden panels initialise at zero width; resize on reveal.
        window.addEventListener("resize", () => {
            for (const c of charts) c.inst.resize();
        });
    }

    /** @param {number} panel */
    const resizePanel = (panel) => {
        for (const c of charts) {
            if (c.panel === panel) c.inst.resize();
        }
    };

    // Tab switching (+ resize the newly revealed panel's charts).
    for (const btn of app.querySelectorAll(".tab")) {
        btn.addEventListener("click", () => {
            const i = btn.getAttribute("data-i");
            for (const b of app.querySelectorAll(".tab")) {
                b.classList.toggle("active", b.getAttribute("data-i") === i);
            }
            for (const p of app.querySelectorAll(".panel")) {
                p.classList.toggle("active", p.getAttribute("data-i") === i);
            }
            if (i !== null) resizePanel(Number(i));
        });
    }

    // Facet dropdowns: a chart is shown only if it matches every non-"All"
    // selection (target AND metric AND warmth, etc.).
    /** @param {HTMLElement} panel */
    const applyFilters = (panel) => {
        const selects = panel.querySelectorAll(".facet-select");
        for (const chart of panel.querySelectorAll(".chart")) {
            if (!(chart instanceof HTMLElement)) continue;
            let show = true;
            for (const sel of selects) {
                if (!(sel instanceof HTMLSelectElement)) continue;
                if (sel.value === "__all__") continue;
                const dim = sel.getAttribute("data-dim");
                if (
                    dim &&
                    chart.getAttribute(`data-facet-${dim}`) !== sel.value
                ) {
                    show = false;
                    break;
                }
            }
            chart.style.display = show ? "" : "none";
        }
        const idx = panel.getAttribute("data-i");
        if (idx !== null) resizePanel(Number(idx));
    };

    for (const sel of app.querySelectorAll(".facet-select")) {
        sel.addEventListener("change", () => {
            const panel = sel.closest(".panel");
            if (panel instanceof HTMLElement) applyFilters(panel);
        });
    }
})();
