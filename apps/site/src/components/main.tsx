import type { ParentProps } from "solid-js";

import { css } from "../../styled-system/css";
import type { SystemStyleObject } from "../../styled-system/types";

// The shared page container styles: the `main` padding plus the styling for
// links rendered inside it (formerly the global `main a` / `a:focus-visible`
// rules). Exported so routes that need to extend it (e.g. the docs page) can
// merge it into a single static `css()` call for a deterministic cascade.
export const mainStyles: SystemStyleObject = {
    padding: "3rem 1rem",
    "& a": {
        color: "#1d5f91",
        fontWeight: 600,
        textDecorationColor: "#7bb6df",
        textDecorationThickness: "0.1em",
        textUnderlineOffset: "0.2em",
        transition: "color 150ms ease",
    },
    "& a:hover": {
        color: "#123f63",
    },
    "& a:focus-visible": {
        outline: "3px solid #0284c7",
        outlineOffset: "3px",
        borderRadius: "0.2rem",
    },
};

export function Main(props: ParentProps) {
    return <main class={css(mainStyles)}>{props.children}</main>;
}
