# Documentation website

The Schedule Everything docs, built with [Docusaurus](https://docusaurus.io/)
and published to https://autophd-org.github.io/schedule-everything/ by
`.github/workflows/deploy-docs.yml`. Dependencies are managed with
[Bun](https://bun.sh); Docusaurus itself runs on Node.js 20 or newer.

```bash
bun install --frozen-lockfile    # install exact dependencies from bun.lock
bun run start                    # local dev server (English)
bun run start --locale zh        # local dev server (Chinese)
bun run build                    # production build for en and zh into build/
bun run serve                    # serve the production build
bun run typecheck                # TypeScript check
```

- Docs pages: `docs/` (Chinese overrides in `i18n/zh/docusaurus-plugin-content-docs/current/`)
- Homepage: `src/pages/index.tsx`; Chinese strings in `i18n/zh/code.json`
- Images and diagrams: `static/img/` (use `<Figure src="img/..." alt="..." />` in docs)
