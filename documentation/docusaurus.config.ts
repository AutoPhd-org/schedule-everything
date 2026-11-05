import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const repoUrl = 'https://github.com/AutoPhd-org/schedule-everything';

const config: Config = {
  title: 'Schedule Everything',
  tagline:
    'Your AI agent plans the week. Persistent reminders keep you on it.',
  favicon: 'img/logo.svg',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
    // `v4: true` also turns on Docusaurus Faster (Rspack/SWC), which needs the
    // optional @docusaurus/faster package. Keep the webpack bundler instead.
    faster: false,
  },

  url: 'https://autophd-org.github.io',
  baseUrl: '/schedule-everything/',

  // GitHub Pages deployment config.
  organizationName: 'AutoPhd-org',
  projectName: 'schedule-everything',

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',

  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'throw',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'zh'],
    localeConfigs: {
      en: {
        label: 'English',
        direction: 'ltr',
      },
      zh: {
        label: '中文',
        direction: 'ltr',
        htmlLang: 'zh-CN',
      },
    },
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: `${repoUrl}/tree/main/documentation/`,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/social-card.jpg',
    metadata: [
      {
        name: 'keywords',
        content:
          'schedule, routine, reminders, pomodoro, TOML, CLI, Claude Code, Codex, Antigravity, local-first',
      },
      {name: 'twitter:card', content: 'summary_large_image'},
    ],
    colorMode: {
      respectPrefersColorScheme: true,
    },
    docs: {
      sidebar: {
        hideable: true,
      },
    },
    navbar: {
      title: 'Schedule Everything',
      hideOnScroll: false,
      logo: {
        alt: 'Schedule Everything logo',
        src: 'img/logo.svg',
        width: 32,
        height: 32,
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'tutorialSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          type: 'doc',
          docId: 'ai-agents',
          position: 'left',
          label: 'AI agents',
        },
        {
          type: 'doc',
          docId: 'browser',
          position: 'left',
          label: 'Browser',
        },
        {
          type: 'doc',
          docId: 'cli/overview',
          position: 'left',
          label: 'CLI',
        },
        {
          type: 'localeDropdown',
          position: 'right',
        },
        {
          href: repoUrl,
          position: 'right',
          className: 'header-github-link',
          'aria-label': 'GitHub repository',
        },
      ],
    },
    footer: {
      style: 'dark',
      logo: {
        alt: 'Schedule Everything logo',
        src: 'img/logo.svg',
        width: 44,
        height: 44,
        href: repoUrl,
      },
      links: [
        {
          title: 'Start',
          items: [
            {label: 'Introduction', to: '/docs/intro'},
            {label: 'Installation', to: '/docs/installation'},
            {label: 'Quick start', to: '/docs/quick-start'},
            {label: 'AI agent plugins', to: '/docs/ai-agents'},
          ],
        },
        {
          title: 'Use it',
          items: [
            {label: 'Browser workspace', to: '/docs/browser'},
            {label: 'Weekly schedules', to: '/docs/configuration/weekly-schedules'},
            {label: 'CLI commands', to: '/docs/cli/overview'},
          ],
        },
        {
          title: 'Community',
          items: [
            {label: 'GitHub', href: repoUrl},
            {label: 'Issues', href: `${repoUrl}/issues`},
            {label: 'PyPI package', href: 'https://pypi.org/project/schedule-management/'},
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Schedule Everything · MIT licensed · Your files stay on your computer.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['toml', 'bash', 'json'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
