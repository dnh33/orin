import { defineConfig } from 'astro/config';

// Static site deployed to GitHub Pages as a project site.
// If the site is ever served from a custom domain at the root, drop `base`.
export default defineConfig({
  base: '/orin',
  trailingSlash: 'ignore',
});
