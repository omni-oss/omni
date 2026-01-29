import { Command } from "@commander-js/extra-typings";
import { add } from "../add";

const command = new Command();

command.action(async () => {
    console.error("No action specified.");
    process.exit(1);
});

command
    .command("add", "Add two numbers.")
    .argument("<a>", "The first number.")
    .argument("<b>", "The second number.")
    .action(async (a, b) => {
        console.log(`result ${add(Number(a), Number(b))}`);
    });

await command.parseAsync();
