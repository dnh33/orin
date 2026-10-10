import { defineConfig } from 'astro/config';

// Static site. Served two ways:
//   - GitHub Pages as a project site (base '/orin', the default).
//   - orin.hjermitslev.dev from Cloudflare Pages at the root. Cloudflare sets
//     CF_PAGES during its builds, so root serving detects itself; ORIN_BASE
//     overrides either case by hand.
// If the GitHub Pages deployment is ever retired, drop `base` entirely.
export default defineConfig({
  base: process.env.CF_PAGES ? '/' : process.env.ORIN_BASE || '/orin',
  trailingSlash: 'ignore',
});
