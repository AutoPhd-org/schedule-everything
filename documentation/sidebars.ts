import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

/**
 * Creating a sidebar enables you to:
 - create an ordered group of docs
 - render a sidebar for each doc of that group
 - provide next/previous navigation

 The sidebars can be generated from the filesystem, or explicitly defined here.

 Create as many sidebars as you want.
 */
const sidebars: SidebarsConfig = {
  // By default, Docusaurus generates a sidebar from the docs folder structure
  tutorialSidebar: [
    {
      type: 'category',
      label: 'Getting Started',
      items: [
        'intro',
        'installation',
        'quick-start',
        'browser',
      ],
    },
    {
      type: 'category',
      label: 'Configuration',
      items: [
        'configuration/overview',
        'configuration/settings',
        'configuration/weekly-schedules',
        'configuration/templates',
      ],
    },
    {
      type: 'category',
      label: 'CLI Commands',
      items: [
        'cli/overview',
        'cli/schedule-management',
        'cli/task-management',
      ],
    },
    {
      type: 'category',
      label: 'Platform Guides',
      items: [
        'platform/macos',
      ],
    },
    {
      type: 'category',
      label: 'Advanced Features',
      items: [
        'advanced/weekly-rotation',
      ],
    },
  ],
};

export default sidebars;
