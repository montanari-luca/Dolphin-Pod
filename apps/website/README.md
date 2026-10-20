# Dolphin Pod website

Static product site built with **Astro + Tailwind CSS + Motion**, managed with **pnpm** as the `@dolphin/website`
workspace package. It reuses the app's design tokens (`@dolphin/shared/tokens.css`), so site and app look alike.

| Task | Command |
| --- | --- |
| Dev server | `pnpm --filter @dolphin/website dev` (http://localhost:4321) |
| Production build | `pnpm --filter @dolphin/website build` (output in `dist/`) |
| Type check | `pnpm --filter @dolphin/website typecheck` |

## How it works

- **Docs and guides** are the Markdown files in the repo's top-level [`docs/`](../../docs) folder, loaded by an Astro
  content collection (`src/content.config.ts`). There are no copies: edit `docs/`, the site follows. The page
  title is the file's first `# Heading`; sidebar order lives in `src/lib/docs.ts`; relative `*.md` links are
  rewritten to site routes by `src/lib/rehype-doc-links.mjs`.
- **Downloads** come from the update manifest (the same file as `UPDATE_MANIFEST_URL` in the apps). It is fetched
  at build time for static links (with a fallback if the fetch fails) and re-fetched in the browser, so a new
  release appears without a rebuild. Platforms with an empty link show "Coming soon".
- **Screenshots**: drop PNGs into `src/assets/screenshots/` (see the README there).
- **Motion** effects are small scripts using the framework-agnostic `motion` API, load only on pages that use
  them, and are off under `prefers-reduced-motion`.
- **Domain**: set the `SITE_URL` env var when building (default `https://dolphinpod.app`) and update the sitemap
  line in `public/robots.txt`.
