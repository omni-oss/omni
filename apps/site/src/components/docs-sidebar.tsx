import { Link } from "@tanstack/solid-router";
import { For, Show } from "solid-js";

import { css } from "../../styled-system/css";
import { buildNav, type NavNode } from "../lib/docs";

const docsSidebar = css({
    position: "sticky",
    top: 0,
    maxHeight: "100vh",
    overflowY: "auto",
    padding: "1.5rem 1rem",
    borderRight: "1px solid #e5e7eb",
    fontSize: "0.9rem",
    "& ul": {
        listStyle: "none",
        margin: 0,
        paddingLeft: "0.75rem",
    },
    "& > ul": {
        paddingLeft: 0,
    },
    "& li": {
        margin: "0.15rem 0",
    },
    "& a": {
        display: "block",
        padding: "0.2rem 0.4rem",
        borderRadius: "0.35rem",
        color: "#334155",
        textDecoration: "none",
    },
    "& a:hover": {
        backgroundColor: "#f1f5f9",
    },
    "& a:focus-visible": {
        outline: "3px solid #0284c7",
        outlineOffset: "3px",
        borderRadius: "0.2rem",
    },
    "& a.active": {
        backgroundColor: "#e0f2fe",
        color: "#0c4a6e",
        fontWeight: 600,
    },
    "& .docs-group": {
        display: "block",
        padding: "0.35rem 0.4rem 0.1rem",
        color: "#94a3b8",
        fontSize: "0.75rem",
        fontWeight: 700,
        textTransform: "uppercase",
        letterSpacing: "0.04em",
    },
});

// The tree is derived once from the (build-time) file layout.
const nav = buildNav();

export function DocsSidebar() {
    return (
        <nav class={docsSidebar} aria-label="Documentation">
            <ul>
                <For each={nav}>{(node) => <NavItem node={node} />}</For>
            </ul>
        </nav>
    );
}

function NavItem(props: { node: NavNode }) {
    return (
        <li>
            <Show
                when={props.node.hasPage}
                fallback={<span class="docs-group">{props.node.label}</span>}
            >
                <Link
                    to="/docs/$"
                    params={{ _splat: props.node.slug }}
                    activeProps={{ class: "active" }}
                >
                    {props.node.label}
                </Link>
            </Show>
            <Show when={props.node.children.length > 0}>
                <ul>
                    <For each={props.node.children}>
                        {(child) => <NavItem node={child} />}
                    </For>
                </ul>
            </Show>
        </li>
    );
}
