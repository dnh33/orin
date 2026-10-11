import { defineConfig } from 'astro/config';

// Static site, served at the root of its own custom domain:
//   - orin.hjermitslev.dev (GitHub Pages, Cloudflare DNS).
// GitHub Pages serves custom-domain sites at the root, so base is '/'.
// ORIN_BASE overrides for any other mount point.
export default defineConfig({
  base: process.env.ORIN_BASE || '/',
  trailingSlash: 'ignore',
});
