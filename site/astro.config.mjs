import { defineConfig } from 'astro/config';

// Static site. Served two ways:
//   - GitHub Pages as a project site (base '/orin', the default).
//   - orin.hjermitslev.dev from Cloudflare Pages at the root (ORIN_BASE='/').
// If the GitHub Pages deployment is ever retired, drop `base` entirely.
export default defineConfig({
  base: process.env.ORIN_BASE || '/orin',
  trailingSlash: 'ignore',
});
