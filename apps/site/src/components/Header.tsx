import { Link } from "@tanstack/solid-router";
import { House, Menu, X } from "lucide-solid";
import { createSignal } from "solid-js";
import { css } from "~/css";
import { flex } from "~/patterns";

export default function Header() {
    const [isOpen, setIsOpen] = createSignal(false);

    // Common nav link style to keep activeProps clean
    const navLinkStyle = css({
        display: "flex",
        alignItems: "center",
        gap: "3",
        p: "3",
        rounded: "lg",
        transition: "colors",
        mb: "2",
        _hover: { bg: "gray.800" },
    });

    const activeNavLinkStyle = css({
        bg: "cyan.600!", // ! is shorthand for !important to override base hover
        _hover: { bg: "cyan.700" },
    });

    return (
        <>
            <header
                class={flex({
                    p: "4",
                    align: "center",
                    bg: "gray.800",
                    color: "white",
                    shadow: "lg",
                })}
            >
                <button
                    type="button"
                    onClick={() => setIsOpen(true)}
                    class={css({
                        p: "2",
                        _hover: { bg: "gray.700" },
                        rounded: "lg",
                        transition: "colors",
                        cursor: "pointer",
                    })}
                    aria-label="Open menu"
                >
                    <Menu size={24} />
                </button>
                <h1
                    class={css({
                        ml: "4",
                        fontSize: "xl",
                        fontWeight: "semibold",
                    })}
                >
                    <Link to="/">
                        <img
                            src="/tanstack-word-logo-white.svg"
                            alt="TanStack Logo"
                            class={css({ h: "10" })}
                        />
                    </Link>
                </h1>
            </header>

            {/* Sidebar Overlay (Optional but recommended for UX) */}
            {isOpen() && (
                <div
                    onPointerDown={() => setIsOpen(false)}
                    class={css({
                        position: "fixed",
                        inset: "0",
                        bg: "black/50",
                        zIndex: "40",
                        backdropBlur: "sm",
                    })}
                />
            )}

            <aside
                class={css({
                    position: "fixed",
                    top: "0",
                    left: "0",
                    h: "full",
                    w: "80",
                    bg: "gray.900",
                    color: "white",
                    shadow: "2xl",
                    zIndex: "50",
                    display: "flex",
                    flexDirection: "column",
                    transition: "transform",
                    transitionDuration: "300ms",
                    transitionTimingFunction: "ease-in-out",
                    transform: isOpen() ? "translateX(0)" : "translateX(-100%)",
                })}
            >
                <div
                    class={flex({
                        align: "center",
                        justify: "space-between",
                        p: "4",
                        borderBottom: "1px solid",
                        borderColor: "gray.700",
                    })}
                >
                    <h2 class={css({ fontSize: "xl", fontWeight: "bold" })}>
                        Navigation
                    </h2>
                    <button
                        type="button"
                        onClick={() => setIsOpen(false)}
                        class={css({
                            p: "2",
                            _hover: { bg: "gray.800" },
                            rounded: "lg",
                            transition: "colors",
                            cursor: "pointer",
                        })}
                        aria-label="Close menu"
                    >
                        <X size={24} />
                    </button>
                </div>

                <nav class={css({ flex: "1", p: "4", overflowY: "auto" })}>
                    <Link
                        to="/"
                        onClick={() => setIsOpen(false)}
                        class={navLinkStyle}
                        activeProps={{
                            class: `${navLinkStyle} ${activeNavLinkStyle}`,
                        }}
                    >
                        <House size={20} />
                        <span class={css({ fontWeight: "medium" })}>Home</span>
                    </Link>
                </nav>
            </aside>
        </>
    );
}
