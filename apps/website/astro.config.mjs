import { defineConfig } from "astro/config";
import mdx from "@astrojs/mdx";
import sitemap from "@astrojs/sitemap";
import { unified } from "@astrojs/markdown-remark";
import tailwindcss from "@tailwindcss/vite";
import { rehypeDocLinks } from "./src/lib/rehype-doc-links.mjs";

// Set SITE_URL once the domain is decided (sitemap and canonical/OG URLs use it).
const site = process.env.SITE_URL ?? "https://dolphinpod.app";

export default defineConfig({
  site,
  output: "static",
  integrations: [mdx(), sitemap({ filter: (page) => !page.endsWith("/docs/") })],
  vite: { plugins: [tailwindcss()] },
  markdown: {
    processor: unified({ rehypePlugins: [rehypeDocLinks] }),
    shikiConfig: {
      themes: { light: "github-light", dark: "github-dark" },
      defaultColor: false,
    },
  },
});
