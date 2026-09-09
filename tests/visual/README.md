# Visual regression

Run `npm ci`, then `npx playwright install chromium`. `npm run test:visual`
compares existing baselines and fails on missing or changed images. Only
`npm run test:visual:update` creates or replaces baselines; review image changes
before committing them.

The suite mounts the real Svelte App with a synthetic bridge, a fixed Date,
disabled polling, disabled animation/caret, zh-CN locale, Asia/Shanghai timezone,
dark color scheme and device scale 1. It captures seven states in both themes:
compact idle/working/error, expanded monitoring, approval, long log and a 360px
single-column layout. Screenshots cover the viewport, including overflow and
the separate approval layer.

Baselines are platform-specific (`chromium-win32`), generated with the Chromium
revision pinned by package-lock.json. No system-Chrome fallback is allowed.
The initial baseline uses Playwright 1.63.0 / Chromium 153.0.8010.12 (revision 1243).
Windows system fonts remain part of the rendering environment: these images
are not portable assertions for Linux/macOS or native WebView2. CI must run on
Windows with the same locked dependencies. Browser or font updates may require
an explicit, visually reviewed baseline update. The suite does not verify
native Windows DPI, monitor switching, window dragging or installer behavior.

Expected images live in `__screenshots__/island.visual.test.ts/`. On failure,
Vitest writes actual/diff images under the ignored `artifacts/` directory.
