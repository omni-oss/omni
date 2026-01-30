import path from "node:path";
import { type System, VirtualSystem } from "@omni-oss/system-interface";
import { describe, expect, it } from "vitest";
import { serialize } from "./codec-utils";
import type { SetVersionConfig } from "./config";
import {
    CONFIG_FILE_NAMES,
    findConfigAtDir,
    NoConfigFoundError,
} from "./find-config";
import { Format } from "./format";

const CONFIG: SetVersionConfig = {
    profiles: [
        {
            type: "path",
            files: ["package.json"],
            path: ["version"],
            format: Format.JSON,
        },
    ],
};

const TEST_PATH = "/home/user/test";

describe("findConfigAtDir", () => {
    it("should throw if required and no config found", async () => {
        const system = await createTempSystem();
        await expect(findConfigAtDir(TEST_PATH, true, system)).rejects.toThrow(
            NoConfigFoundError,
        );
    });

    it("should not throw if not required and no config found", async () => {
        const system = await createTempSystem();
        await expect(
            findConfigAtDir(TEST_PATH, false, system),
        ).resolves.toBeUndefined();
    });

    it.each(
        CONFIG_FILE_NAMES,
    )("should parse config file (%s)", async (configName) => {
        const system = await createTempSystem(configName);
        const config = await findConfigAtDir(TEST_PATH, true, system);

        expect(config).toEqual(CONFIG);
    });
});

async function createTempSystem(configName?: string): Promise<System> {
    const system = await VirtualSystem.create();

    await system.fs.createDirectory(TEST_PATH, { recursive: true });

    if (configName) {
        await system.fs.writeStringToFile(
            path.join(TEST_PATH, configName),
            serialize(configName, CONFIG),
        );
    }

    return system;
}
