import { defineConfig } from 'vitepress'

const guide = [
  { text: 'Installation', link: '/guide/installation' },
  { text: 'Configuration', link: '/guide/configuration' },
  { text: 'CLI', link: '/guide/cli' },
  { text: 'Rust library', link: '/guide/rust-library' },
  { text: 'Docker', link: '/guide/docker' },
  { text: 'Commissions', link: '/guide/commissions' }
]

export default defineConfig({
  title: 'Gulf',
  description: 'Config-driven bridge and swap routing',
  base: '/gulf/',
  cleanUrls: true,
  head: [
    ['link', { rel: 'canonical', href: 'https://spilneo.github.io/gulf/' }],
    ['meta', { name: 'theme-color', content: '#0a5964' }]
  ],
  themeConfig: {
    nav: [
      { text: 'Guide', items: guide },
      { text: 'Concepts', link: '/concepts/architecture' },
      { text: 'Reference', link: '/reference/http-api' },
      { text: 'Operations', link: '/operations/live-testing' },
      { text: 'GitHub', link: 'https://github.com/spilneo/gulf' }
    ],
    sidebar: [
      { text: 'Guide', items: guide },
      { text: 'Concepts', items: [
        { text: 'Architecture', link: '/concepts/architecture' },
        { text: 'Providers', link: '/concepts/providers' }
      ] },
      { text: 'Reference', items: [
        { text: 'HTTP API', link: '/reference/http-api' },
        { text: 'JSON responses', link: '/reference/json-responses' },
        { text: 'Errors', link: '/reference/errors' }
      ] },
      { text: 'Operations', items: [
        { text: 'Live testing', link: '/operations/live-testing' },
        { text: 'Security', link: '/operations/security' }
      ] },
      { text: 'Project', items: [{ text: 'Contributing', link: '/contributing' }] }
    ],
    search: { provider: 'local' },
    outline: { level: [2, 3], label: 'On this page' },
    socialLinks: [{ icon: 'github', link: 'https://github.com/spilneo/gulf' }],
    footer: { message: 'Apache-2.0 OR MIT', copyright: 'Copyright © 2026 Gulf contributors' }
  }
})
