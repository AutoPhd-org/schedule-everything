# Documentation website

The Schedule Everything docs, built with [Docusaurus](https://docusaurus.io/)
and published to https://autophd-org.github.io/schedule-everything/ by
`.github/workflows/deploy-docs.yml`. Requires Node.js 20 or newer.

```bash
npm ci              # install exact dependencies from package-lock.json
npm start           # local dev server (English)
npm start -- --locale zh   # local dev server (Chinese)
npm run build       # production build for en and zh into build/
npm run serve       # serve the production build
npm run typecheck   # TypeScript check
```

- Docs pages: `docs/` (Chinese overrides in `i18n/zh/docusaurus-plugin-content-docs/current/`)
- Homepage: `src/pages/index.tsx`; Chinese strings in `i18n/zh/code.json`
- Images and diagrams: `static/img/` (use `<Figure src="img/..." alt="..." />` in docs)
