import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

// The docs sidebar, in reading order.
const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    {
      type: 'category',
      label: 'Getting Started',
      collapsible: false,
      items: ['intro', 'installation', 'quick-start', 'ai-agents', 'browser'],
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
        'cli/deadline-management',
      ],
    },
    {
      type: 'category',
      label: 'Platform Guides',
      items: ['platform/macos', 'platform/linux'],
    },
    {
      type: 'category',
      label: 'Advanced Features',
      items: ['advanced/weekly-rotation', 'advanced/launchx'],
    },
  ],
};

export default sidebars;
