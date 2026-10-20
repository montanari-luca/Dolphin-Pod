# Phase 3: Website

> Temporary plan. Delete the `implementation/` directory once all phases are complete.

**Goal:** a fast, static product site in the same frosted-glass style as the app, with a home page, features,
screenshots, documentation, guides, downloads and credits.

**Stack:** **Astro + Tailwind CSS + Motion**, managed with **pnpm** as the `@dolphin/website` workspace package
in `apps/website/` (it joins Turborepo's `build` / `lint` / `typecheck` tasks automatically).

**Status:** built and committed. Carry-overs before launch (everything else below is ticked):
- Capture real 2x screenshots into `apps/website/src/assets/screenshots/` (see its README); until then the
  Screenshots page shows the built-in illustration.
- Run Lighthouse on the home and docs pages (not run yet: no Chrome available when this was built). Contrast
  fixes for both themes and reduced motion are already in.
- Features page: add iPhone/remote-control details and the sync and iPhone guides once Phase 2 ships.
- Decide the open questions at the bottom (domain: set `SITE_URL` and `public/robots.txt`; license wording; analytics).

**Exit criteria**
- `pnpm --filter @dolphin/website build` produces a static site with no client JS on pages that don't need it.
- Download buttons always match the update manifest (same versions and links as the apps).
- Docs pages render from `docs/` with no copy-paste duplication.
- Lighthouse ≥ 95 for performance and accessibility on the home and docs pages; reduced-motion respected.

---

## 1. Scaffold

- [x] `apps/website/package.json` (`@dolphin/website`, `type: module`) with `astro` (7.x), `tailwindcss` +
      `@tailwindcss/vite` (4.x), `motion` (14.x), `@astrojs/mdx`, `@astrojs/sitemap`; scripts `dev`, `build`,
      `preview`, `typecheck` (`astro check`).
- [x] `astro.config.mjs`: `output: "static"`, Tailwind through the Vite plugin, MDX, sitemap, `site` set once the
      domain is known.
- [x] Reuse the design tokens: `@import "@dolphin/shared/tokens.css"` in the global stylesheet and map them in
      `@theme inline` exactly like `apps/desktop/src/index.css` (same color, radius and glass utilities), so the
      site and the app look like one product. Fonts: `@fontsource-variable/inter` and `@fontsource/jetbrains-mono`.
- [x] `public/` already contains the favicons, apple-touch icon, PWA icons (incl. maskable) and `og-image.png`;
      add `site.webmanifest` and the `<head>` tags in a shared `BaseLayout.astro`.
- [x] Theme: system, dark and light with a toggle (inline script in `<head>` to avoid a flash, stored in
      `localStorage` with a try/catch).

## 2. Motion

- [x] Use the framework-agnostic `motion` API (`animate`, `inView`, `scroll`, `stagger`) in small `<script>`
      blocks: section reveals, hero glass panels floating, a staggered agent roster. No React runtime needed.
- [x] If an interactive island ever needs React, add `@astrojs/react` and use `motion/react` there only.
- [x] Gate every animation on `prefers-reduced-motion` (and keep content visible without JS).

## 3. Pages

- [x] **Home**: hero (logo, "Your pod of AI agents, in one chat", download CTA for the detected platform), a
      product screenshot in a glass frame, how routing works (Roger → Lincoln → specialist, plus @mentions), the
      agent roster with their colors, provider logos (local and cloud), privacy points (local-first, keys in the
      keychain, encrypted sync).
- [x] **Features**: chats and agents, routing and modes, plans and permissions, streamed thinking, preferences
      efficiency, cleanup and automations, hover cards and usage, iPhone and remote control (once Phase 2 ships).
- [ ] **Screenshots** (page + auto-picking gallery done; real captures still to add): a gallery (dark and light). Capture from the real app at 2x and store them in
      `src/assets/screenshots/` so Astro's `<Image>` optimizes them (AVIF/WebP).
- [x] **Docs**: an Astro content collection reading `../../docs/**/*.md` (getting started, agents, providers,
      routing, permissions) with a sidebar, previous/next links and an "Edit on GitHub" link.
- [x] **Guides**: `docs/guides/*.md` (Ollama, LM Studio, OpenRouter done; sync setup and iPhone remote control wait for Phase 2).
- [x] **Download**: reads the update manifest
      (`https://raw.githubusercontent.com/AstroDevCompany/MintFlow-Releases/refs/heads/main/dolphinpod.json`, the
      same file as `UPDATE_MANIFEST_URL` in the apps). Platforms with an empty link show "Coming soon". Fetch it
      at build time for static links, and re-fetch on the client so a new release shows without a rebuild. Show
      the version, per-platform buttons (macOS, Windows, Linux, iOS) and system requirements.
- [x] **Footer on every page**: MintFlow Technologies (mintflow.dev, logo from `packages/brand/source/`), lead
      developer Luca Montanari (instagram.com/montanarii.luca), © 2026 MintFlow Technologies, license note.
- [x] 404 page in the same style.

## 4. Quality

- [x] SEO: titles and descriptions per page, Open Graph and Twitter tags with `og-image.png`, sitemap,
      `robots.txt`.
- [x] Accessibility: semantic landmarks, focus styles from the tokens, contrast checked in both themes, alt
      text on every screenshot.
- [x] CI: add the website build to `.github/workflows/ci.yml` (covered automatically by `pnpm build` through
      Turborepo).

## Open questions

1. Hosting and domain (e.g. Cloudflare Pages, Netlify or Vercel; `dolphinpod.app` or a `mintflow.dev` path?).
2. License wording for the site footer (the repo is currently "All rights reserved").
3. Analytics: none, or a privacy-friendly option?
