#!/usr/bin/env bun
import { Command } from "@commander-js/extra-typings";
import { description, name, version } from "../../package.json";
import { add } from "../add";

const program = new Command();

program.name(name).version(version).description(description);

program
    .command("add")
    .description("Add two numbers.")
    .argument("<a>", "The first number.")
    .argument("<b>", "The second number.")
    .action(async (a, b) => {
        console.log(`result ${add(Number(a), Number(b))}`);
    });

program.parseAsync();
