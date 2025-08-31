import { createFileRoute, Link } from "@tanstack/react-router";
import { HomeLayout } from "fumadocs-ui/layouts/home";
import { baseOptions } from "@/lib/layout.shared";

export const Route = createFileRoute("/")({
    component: Home,
});

function Home() {
    return (
        <HomeLayout
            {...baseOptions()}
            links={[
                {
                    type: "main",
                    on: "all",
                    text: "Documentation",
                    url: "/docs/quick-start",
                },
            ]}
            className="text-center pt-32 justify-center"
            about="Omni Documentation"
        >
            {/* Hero Section */}
            <main className="container mx-auto px-6 md:px-16 py-32 text-center">
                <div className="max-w-4xl mx-auto">
                    <h1 className="text-5xl md:text-7xl font-extrabold leading-tight text-white mb-6 animate-fade-in">
                        The Unified Build System for Modern{" "}
                        <span className="text-blue-500">Monorepos</span>
                    </h1>
                    <p className="text-lg md:text-xl text-gray-400 mb-10 animate-fade-in delay-200">
                        Omni helps you manage and scale complex projects by
                        providing a powerful, integrated toolkit for your
                        monorepo.
                    </p>
                    <div className="flex flex-col md:flex-row justify-center space-y-4 md:space-y-0 md:space-x-4">
                        <Link
                            to="/docs/$"
                            params={{
                                _splat: "quick-start",
                            }}
                            className="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 bg-blue-500 text-white shadow hover:bg-blue-600 h-10 px-6 py-2 transform transition-transform hover:scale-105"
                        >
                            Get Started
                        </Link>
                        <Link
                            to="/docs/$"
                            params={{
                                _splat: "features",
                            }}
                            className="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 border border-input bg-background hover:bg-gray-800 hover:text-gray-50 h-10 px-6 py-2 border-gray-700 text-gray-400"
                        >
                            Learn More
                        </Link>
                    </div>
                </div>
            </main>

            {/* Features Section */}
            <section className="py-24 px-6 md:px-16">
                <div className="container mx-auto">
                    <h2 className="text-4xl md:text-5xl font-bold text-center text-white mb-16">
                        Core Features
                    </h2>
                    <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
                        {/* Feature Card 1: Smart Task Execution & Caching */}
                        <div className="relative rounded-2xl border border-gray-800 bg-gray-950 p-6 shadow-lg hover:shadow-xl transition-shadow duration-300">
                            <div className="flex flex-col items-center text-center">
                                <div className="w-16 h-16 flex items-center justify-center bg-blue-600 text-white rounded-full mb-4">
                                    <svg
                                        xmlns="http://www.w3.org/2000/svg"
                                        className="h-8 w-8"
                                        fill="none"
                                        viewBox="0 0 24 24"
                                        stroke="currentColor"
                                        strokeWidth={2}
                                    >
                                        <path
                                            strokeLinecap="round"
                                            strokeLinejoin="round"
                                            d="M13 10V3L4 14h7v7l9-11h-7z"
                                        />
                                    </svg>
                                </div>
                                <h3 className="text-xl font-semibold text-white mb-2">
                                    Smart Task Execution
                                </h3>
                                <p className="text-gray-400 leading-relaxed">
                                    Omni's advanced caching ensures that tasks
                                    only run on affected projects, saving you
                                    valuable time on builds and tests.
                                </p>
                            </div>
                        </div>
                        {/* Feature Card 2: Powerful Code Generation */}
                        <div className="relative rounded-2xl border border-gray-800 bg-gray-950 p-6 shadow-lg hover:shadow-xl transition-shadow duration-300">
                            <div className="flex flex-col items-center text-center">
                                <div className="w-16 h-16 flex items-center justify-center bg-blue-600 text-white rounded-full mb-4">
                                    <svg
                                        xmlns="http://www.w3.org/2000/svg"
                                        className="h-8 w-8"
                                        fill="none"
                                        viewBox="0 0 24 24"
                                        stroke="currentColor"
                                        strokeWidth={2}
                                    >
                                        <path
                                            strokeLinecap="round"
                                            strokeLinejoin="round"
                                            d="M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 21v-4a4 4 0 014-4h4a4 4 0 014 4v4h-6z"
                                        />
                                    </svg>
                                </div>
                                <h3 className="text-xl font-semibold text-white mb-2">
                                    Powerful Code Generation
                                </h3>
                                <p className="text-gray-400 leading-relaxed">
                                    Quickly generate new libraries, components,
                                    and boilerplate code with flexible, built-in
                                    schematics.
                                </p>
                            </div>
                        </div>
                        {/* Feature Card 3: Interactive Project Graph */}
                        <div className="relative rounded-2xl border border-gray-800 bg-gray-950 p-6 shadow-lg hover:shadow-xl transition-shadow duration-300">
                            <div className="flex flex-col items-center text-center">
                                <div className="w-16 h-16 flex items-center justify-center bg-blue-600 text-white rounded-full mb-4">
                                    <svg
                                        xmlns="http://www.w3.org/2000/svg"
                                        className="h-8 w-8"
                                        fill="none"
                                        viewBox="0 0 24 24"
                                        stroke="currentColor"
                                        strokeWidth={2}
                                    >
                                        <path
                                            strokeLinecap="round"
                                            strokeLinejoin="round"
                                            d="M3 10v11m6-9v9m6-10v10m6-6v6a2 2 0 01-2 2h-4a2 2 0 01-2-2v-6a2 2 0 012-2h4a2 2 0 012 2zM3 10V4.5a2 2 0 012-2h14a2 2 0 012 2V10"
                                        />
                                    </svg>
                                </div>
                                <h3 className="text-xl font-semibold text-white mb-2">
                                    Interactive Project Graph
                                </h3>
                                <p className="text-gray-400 leading-relaxed">
                                    Visualize your workspace dependencies and
                                    understand the relationship between your
                                    projects with a dynamic graph.
                                </p>
                            </div>
                        </div>
                    </div>
                </div>
            </section>
            {/* Footer */}
            <footer className="py-10 px-6 md:px-16 border-t">
                <div className="container mx-auto flex flex-col md:flex-row items-center justify-between space-y-4 md:space-y-0 text-gray-400 text-sm">
                    <p>
                        &copy; {new Date().getFullYear()} Omni CLI. All rights
                        reserved.
                    </p>
                    <div className="flex space-x-6">
                        <Link
                            to="/docs/$"
                            params={{
                                _splat: "quick-start",
                            }}
                            className="hover:text-white transition-colors"
                        >
                            Docs
                        </Link>
                        <a
                            href="https://github.com/omni-oss/omni"
                            className="hover:text-white transition-colors"
                            target="_blank"
                            rel="noreferrer"
                        >
                            GitHub
                        </a>
                    </div>
                </div>
            </footer>
        </HomeLayout>
    );
}
