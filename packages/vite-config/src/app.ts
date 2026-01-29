import { mergeConfig, type UserConfig } from "vite";
import base from "./base.ts";

export default mergeConfig(base, {} satisfies UserConfig);
