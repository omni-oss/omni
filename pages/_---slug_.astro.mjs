import { c as createComponent, r as renderComponent, b as renderTemplate } from '../chunks/astro/server_B9FNj4Lf.mjs';
import 'kleur/colors';
import { $ as $$Common, p as paths } from '../chunks/common_CMgpYODz.mjs';
export { renderers } from '../renderers.mjs';

const prerender = true;
async function getStaticPaths() {
  return paths;
}
const $$Index = createComponent(($$result, $$props, $$slots) => {
  return renderTemplate`${renderComponent($$result, "CommonPage", $$Common, {})}`;
}, "/home/runner/work/omni/omni/node_modules/@astrojs/starlight/routes/static/index.astro", void 0);

const $$file = "/home/runner/work/omni/omni/node_modules/@astrojs/starlight/routes/static/index.astro";
const $$url = undefined;

const _page = /*#__PURE__*/Object.freeze(/*#__PURE__*/Object.defineProperty({
	__proto__: null,
	default: $$Index,
	file: $$file,
	getStaticPaths,
	prerender,
	url: $$url
}, Symbol.toStringTag, { value: 'Module' }));

const page = () => _page;

export { page };
