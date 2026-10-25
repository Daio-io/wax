/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'wax',
  tagline: 'Design system coverage analytics for agents',
  url: 'https://daio-io.github.io',
  baseUrl: '/wax/',
  organizationName: 'Daio-io',
  projectName: 'wax',
  trailingSlash: true,
  onBrokenLinks: 'throw',
  future: {v4: true},
  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.js',
          editUrl: 'https://github.com/Daio-io/wax/edit/main/website/',
        },
        blog: false,
        theme: {customCss: './src/css/custom.css'},
      },
    ],
  ],
  themeConfig: {
    navbar: {
      title: 'wax',
      items: [
        {
          to: '/docs/getting-started/',
          label: 'Docs',
          position: 'left',
        },
        {
          to: '/docs/reference/cli/',
          label: 'CLI',
          position: 'left',
        },
        {
          to: '/docs/skills/',
          label: 'Skills',
          position: 'left',
        },
        {
          href: 'https://github.com/Daio-io/wax',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Get started', to: '/docs/getting-started/'},
            {label: 'CLI reference', to: '/docs/reference/cli/'},
            {label: 'Agent skills', to: '/docs/skills/'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'GitHub', href: 'https://github.com/Daio-io/wax'},
            {
              label: 'Releases',
              href: 'https://github.com/Daio-io/wax/releases',
            },
          ],
        },
      ],
    },
  },
};

export default config;
