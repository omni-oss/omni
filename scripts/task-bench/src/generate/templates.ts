import type { HarnessConfig } from "../config";
import type { ProjectNode } from "../graph";

/**
 * The minimal source file each project ships. It exists so that every runner
 * has a real input file to hash for cache keys.
 */
export function sourceFile(project: ProjectNode): string {
    return [
        `// Auto-generated source for ${project.name}.`,
        `// Edit the harness config, not this file.`,
        `export const id = ${JSON.stringify(project.name)};`,
        `export const index = ${project.index};`,
        `export const answer = 42;`,
        "",
    ].join("\n");
}

/**
 * The task runner every project uses. It is intentionally tiny but does real
 * work that exercises each runner's caching + log-capture machinery:
 *   - reads its own source (a cache input),
 *   - performs a deterministic CPU loop,
 *   - prints a configurable number of log lines to stdout,
 *   - writes deterministic output file(s) into dist/ (cache outputs).
 *
 * Determinism matters: identical inputs => identical outputs => cache hits on
 * warm runs, which is exactly what we want to measure.
 */
export function taskRunner(
    config: HarnessConfig,
    project: ProjectNode,
): string {
    const { logLines, workIterations, outputFiles } = config.task;
    return `#!/usr/bin/env node
import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const PROJECT = ${JSON.stringify(project.name)};
const LOG_LINES = ${logLines};
const WORK_ITERATIONS = ${workIterations};
const OUTPUT_FILES = ${outputFiles};

const task = process.argv[2] ?? "task";

// Ground-truth execution marker: this line is only ever reached when the task
// actually runs. Cache hits skip the process entirely, so counting the lines
// in this out-of-tree log yields a tool-agnostic cache-hit rate. The log path
// lives outside the workspace so no runner hashes it as an input.
const execLog = process.env.TASK_BENCH_EXEC_LOG;
if (execLog) {
    appendFileSync(execLog, \`\${PROJECT}\\t\${task}\\n\`);
}

const source = readFileSync(join(HERE, "src", "index.js"), "utf8");

// Cheap, deterministic CPU work.
let acc = 0;
for (let i = 0; i < WORK_ITERATIONS; i++) {
    acc = (acc + Math.imul(i ^ acc, 2654435761)) >>> 0;
}

const digest = createHash("sha256")
    .update(source)
    .update(task)
    .update(String(acc))
    .digest("hex");

for (let i = 1; i <= LOG_LINES; i++) {
    console.log(\`[\${PROJECT}] \${task}: step \${i}/\${LOG_LINES} digest=\${digest.slice(0, 12)}\`);
}

const outDir = join(HERE, "dist");
mkdirSync(outDir, { recursive: true });
for (let f = 0; f < OUTPUT_FILES; f++) {
    writeFileSync(
        join(outDir, \`\${task}.\${f}.txt\`),
        \`\${PROJECT}\\t\${task}\\t\${digest}\\n\`,
    );
}

console.log(\`[\${PROJECT}] \${task}: complete (\${OUTPUT_FILES} output file(s))\`);
`;
}
