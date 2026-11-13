import { defineConfig } from "astro/config";
import mdx from "@astrojs/mdx";
import sitemap from "@astrojs/sitemap";
import { unified } from "@astrojs/markdown-remark";
import tailwindcss from "@tailwindcss/vite";
import { rehypeDocLinks } from "./src/lib/rehype-doc-links.mjs";

// Production origin: canonical, Open Graph and sitemap URLs are built from it. SITE_URL overrides it for previews.
const site = process.env.SITE_URL ?? "https://dolphin.mintflow.dev";

export default defineConfig({
  site,
  output: "static",
  integrations: [mdx(), sitemap({ filter: (page) => !page.endsWith("/docs/") && !/\/404\/?$/.test(page) })],
  vite: { plugins: [tailwindcss()] },
  markdown: {
    processor: unified({ rehypePlugins: [rehypeDocLinks] }),
    shikiConfig: {
      themes: { light: "github-light", dark: "github-dark" },
      defaultColor: false,
    },
  },
});
