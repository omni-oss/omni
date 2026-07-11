/**
 * Ambient declarations for Vite `?raw` imports: importing a file with the
 * `?raw` suffix yields its contents as a string. Used by the HTML renderer to
 * inline its client script, styles, and template. See src/render/html.
 */
declare module "*?raw" {
    const content: string;
    export default content;
}
