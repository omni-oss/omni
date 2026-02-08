import { createFileRoute } from "@tanstack/solid-router";
import {
    Route as RouteIcon,
    Server,
    Shield,
    Sparkles,
    Waves,
    Zap,
} from "lucide-solid";
import { For } from "solid-js";
import { css } from "~/css";
import { center, container, grid, stack } from "~/patterns";

export const Route = createFileRoute("/")({ component: App });

// Reusable icon style
const iconStyle = css({
    w: "12",
    h: "12",
    color: "cyan.400",
});

function App() {
    const features = [
        {
            icon: <Zap class={iconStyle} />,
            title: "Powerful Server Functions",
            description:
                "Write server-side code that seamlessly integrates with your client components. Type-safe, secure, and simple.",
        },
        {
            icon: <Server class={iconStyle} />,
            title: "Flexible Server Side Rendering",
            description:
                "Full-document SSR, streaming, and progressive enhancement out of the box. Control exactly what renders where.",
        },
        {
            icon: <RouteIcon class={iconStyle} />,
            title: "API Routes",
            description:
                "Build type-safe API endpoints alongside your application. No separate backend needed.",
        },
        {
            icon: <Shield class={iconStyle} />,
            title: "Strongly Typed Everything",
            description:
                "End-to-end type safety from server to client. Catch errors before they reach production.",
        },
        {
            icon: <Waves class={iconStyle} />,
            title: "Full Streaming Support",
            description:
                "Stream data from server to client progressively. Perfect for AI applications and real-time updates.",
        },
        {
            icon: <Sparkles class={iconStyle} />,
            title: "Next Generation Ready",
            description:
                "Built from the ground up for modern web applications. Deploy anywhere JavaScript runs.",
        },
    ];

    return (
        <div
            class={css({
                minH: "screen",
                bgGradient: "to-b",
                gradientFrom: "slate.900",
                gradientVia: "slate.800",
                gradientTo: "slate.900",
            })}
        >
            {/* Hero Section */}
            <section
                class={css({
                    position: "relative",
                    py: "20",
                    px: "6",
                    textAlign: "center",
                    overflow: "hidden",
                })}
            >
                <div
                    class={css({
                        position: "absolute",
                        inset: "0",
                        bgGradient: "to-r",
                        gradientFrom: "cyan.500/10",
                        gradientVia: "blue.500/10",
                        gradientTo: "purple.500/10",
                    })}
                />

                <div
                    class={stack({
                        position: "relative",
                        maxW: "5xl",
                        mx: "auto",
                        gap: "8",
                    })}
                >
                    <div
                        class={center({
                            gap: "6",
                            flexDirection: { base: "column", md: "row" },
                        })}
                    >
                        <img
                            src="/tanstack-circle-logo.png"
                            alt="TanStack Logo"
                            class={css({
                                w: { base: "24", md: "32" },
                                h: { base: "24", md: "32" },
                            })}
                        />
                        <h1
                            class={css({
                                textStyle: "6xl",
                                md: { textStyle: "7xl" },
                                fontWeight: "black",
                                color: "white",
                            })}
                        >
                            <span class={css({ color: "gray.300" })}>
                                TANSTACK
                            </span>{" "}
                            <span
                                class={css({
                                    bgGradient: "to-r",
                                    gradientFrom: "cyan.400",
                                    gradientTo: "blue.400",
                                    bgClip: "text",
                                    color: "transparent",
                                })}
                            >
                                START
                            </span>
                        </h1>
                    </div>

                    <p
                        class={css({
                            fontSize: { base: "2xl", md: "3xl" },
                            color: "gray.300",
                            fontWeight: "light",
                        })}
                    >
                        The framework for next generation AI applications
                    </p>

                    <p
                        class={css({
                            fontSize: "lg",
                            color: "gray.400",
                            maxW: "3xl",
                            mx: "auto",
                        })}
                    >
                        Full-stack framework powered by TanStack Router for
                        React and Solid. Build modern applications with server
                        functions, streaming, and type safety.
                    </p>

                    <div class={stack({ alignItems: "center", gap: "4" })}>
                        <a
                            href="https://tanstack.com/start"
                            target="_blank"
                            rel="noopener noreferrer"
                            class={css({
                                px: "8",
                                py: "3",
                                bg: "cyan.500",
                                _hover: { bg: "cyan.600" },
                                color: "white",
                                fontWeight: "semibold",
                                rounded: "lg",
                                transition: "colors",
                                shadow: "0 10px 15px -3px rgba(6, 182, 212, 0.5)",
                            })}
                        >
                            Documentation
                        </a>
                        <p class={css({ color: "gray.400", fontSize: "sm" })}>
                            Begin your TanStack Start journey by editing{" "}
                            <code
                                class={css({
                                    px: "2",
                                    py: "1",
                                    bg: "slate.700",
                                    rounded: "sm",
                                    color: "cyan.400",
                                })}
                            >
                                /src/routes/index.tsx
                            </code>
                        </p>
                    </div>
                </div>
            </section>

            {/* Features Grid */}
            <section class={container({ py: "16", px: "6", maxW: "7xl" })}>
                <div
                    class={grid({
                        columns: { base: 1, md: 2, lg: 3 },
                        gap: "6",
                    })}
                >
                    <For each={features}>
                        {(feature) => (
                            <div
                                class={css({
                                    bg: "slate.800/50",
                                    backdropBlur: "sm",
                                    border: "1px solid",
                                    borderColor: "slate.700",
                                    rounded: "xl",
                                    p: "6",
                                    transition: "all",
                                    transitionDuration: "300ms",
                                    _hover: {
                                        borderColor: "cyan.500/50",
                                        shadow: "0 10px 15px -3px rgba(6, 182, 212, 0.1)",
                                    },
                                })}
                            >
                                <div class={css({ mb: "4" })}>
                                    {feature.icon}
                                </div>
                                <h3
                                    class={css({
                                        fontSize: "xl",
                                        fontWeight: "semibold",
                                        color: "white",
                                        mb: "3",
                                    })}
                                >
                                    {feature.title}
                                </h3>
                                <p
                                    class={css({
                                        color: "gray.400",
                                        lineHeight: "relaxed",
                                    })}
                                >
                                    {feature.description}
                                </p>
                            </div>
                        )}
                    </For>
                </div>
            </section>
        </div>
    );
}
