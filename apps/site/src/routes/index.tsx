import { createFileRoute } from "@tanstack/solid-router";

import { css } from "../../styled-system/css";
import { Main } from "../components/main";
import logo from "../logo.svg";

const logoStyle = css({
    animation: "logo-spin infinite 20s linear",
    height: "24vmin",
    pointerEvents: "none",
});

export const Route = createFileRoute("/")({
    head: () => ({ meta: [{ title: "Home - Solid App" }] }),
    component: Home,
});

function Home() {
    return (
        <Main>
            <img src={logo} class={logoStyle} alt="Solid logo" />
            <h1>Hello Solid!</h1>
            <a
                href="https://v2.solidjs.com/"
                target="_blank"
                rel="noopener noreferrer"
            >
                Learn Solid
            </a>
        </Main>
    );
}
