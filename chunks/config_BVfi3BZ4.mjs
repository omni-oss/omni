const astroConfig = {"base":"/","root":"file:///home/runner/work/omni/omni/docs/dev-docs/","srcDir":"file:///home/runner/work/omni/omni/docs/dev-docs/src/","build":{"assets":"_astro"},"markdown":{"shikiConfig":{"langs":[]}}};
const ecIntegrationOptions = {"frames":{"extractFileNameFromCode":false},"styleOverrides":{"borderColor":"var(--fb-code-block-bg-color)","borderRadius":"0.4rem","codeBackground":"var(--fb-code-block-bg-color)","frames":{"editorActiveTabIndicatorBottomColor":"var(--sl-color-gray-3)","editorActiveTabIndicatorTopColor":"unset","editorTabBarBorderBottomColor":"var(--fb-code-block-bg-color)","frameBoxShadowCssValue":"unset","shadowColor":"var(--sl-shadow-sm)"}},"themes":["github-dark-high-contrast","light-plus"]};
let ecConfigFileOptions = {};
try {
	ecConfigFileOptions = (await import('./ec-config_CzTTOeiV.mjs')).default;
} catch (e) {
	console.error('*** Failed to load Expressive Code config file "file:///home/runner/work/omni/omni/docs/dev-docs/ec.config.mjs". You can ignore this message if you just renamed/removed the file.\n\n(Full error message: "' + (e?.message || e) + '")\n');
}

export { astroConfig, ecConfigFileOptions, ecIntegrationOptions };
