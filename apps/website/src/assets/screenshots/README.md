# Screenshots

Drop real app captures here and the Screenshots page picks them up automatically (Astro optimizes them to
AVIF/WebP). Nothing else to edit.

- Naming: `<theme>-<slug>.png`, where theme is `dark` or `light` (for example `dark-chat.png`, `light-chat.png`).
  Slugs with a description in `src/pages/screenshots.astro` (`chat`, `agents`, `providers`, `plans`, `settings`,
  `automations`) get a proper alt text; any other slug falls back to a generic one, so add a line there for new ones.
- Capture at 2x (for example a 1440x900 window saved as 2880x1800), without the desktop wallpaper or other windows.
- Use the in-browser mock (`pnpm --filter @dolphin/desktop dev`, http://localhost:1420) or the real app; use
  sample data only, never real chats or keys.
