import fsAsync from "node:fs/promises";
import { Command } from "@commander-js/extra-typings";
import { TaskResultArraySchema } from "./schemas";
import { summarize } from "./summarizer";

const command = new Command();

await command
    .argument("<input>", "The input file to read from.")
    .option("-o, --output <output>", "The output file to write to.")
    .action(async (input, options) => {
        const inputFile = await fsAsync.readFile(input, "utf-8");
        const results = JSON.parse(inputFile);
        const result = TaskResultArraySchema.safeParse(results);

        if (result.success) {
            const data = result.data;
            const summary = summarize(data);
            if (options.output) {
                await fsAsync.writeFile(
                    options.output,
                    JSON.stringify(summary, null, 2),
                );
            } else {
                console.log(summary);
            }
        } else {
            console.error(result.error);
            process.exit(1);
        }
    })
    .parseAsync();
