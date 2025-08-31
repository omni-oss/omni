import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";

export function baseOptions(): BaseLayoutProps {
    return {
        githubUrl: "https://github.com/omni-oss/omni",
        links: [],
        nav: {
            title: "Omni",
            url: "/",
        },
    };
}
