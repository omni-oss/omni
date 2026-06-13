// Generator fixture — only named exports, no default. Should be rejected by
// the generator runner's "must export a default function" check.
export const meta = "no-default";
export function helper() {
    return 1;
}
