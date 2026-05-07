import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'gRPC Testify',
  description: 'Automate gRPC testing with simple .gctf files',
  ignoreDeadLinks: false,

  base: '/grpctestify-rust/',
  
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/grpctestify-rust/icon.svg' }],
    ['meta', { name: 'theme-color', content: '#667eea' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'gRPC Testify' }]
  ],

  themeConfig: {
    logo: {
      light: '/icon.svg',
      dark: '/icon.svg'
    },

    siteTitle: 'gRPC Testify',

    nav: [
      { text: 'Home', link: '/' },
      { text: 'Get Started', link: '/guides/getting-started/installation' },
      { text: 'Test Files', link: '/guides/reference/api/test-files' },
      { text: 'CLI', link: '/guides/reference/api/command-line' },
      { text: 'Sections', link: '/guides/reference/sections/' }
    ],

    sidebar: {
      '/guides/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Overview', link: '/guides/' },
            { text: 'Installation', link: '/guides/getting-started/installation' },
            { text: 'First Test', link: '/guides/getting-started/first-test' },
            { text: 'Basic Concepts', link: '/guides/getting-started/basic-concepts' },
            { text: 'Troubleshooting', link: '/guides/troubleshooting' }
          ]
        },
        {
          text: 'CLI and Workflow',
          items: [
            { text: 'API Overview', link: '/guides/reference/api/' },
            { text: 'Command Line', link: '/guides/reference/api/command-line' },
            { text: 'Report Formats', link: '/guides/reference/api/report-formats' },
            { text: 'Data Source Framework', link: '/guides/bench-sources' }
          ]
        },
        {
          text: 'Test Format',
          items: [
            { text: 'Test Files', link: '/guides/reference/api/test-files' },
            { text: 'Assertions', link: '/guides/reference/api/assertions' },
            { text: 'Plugin System', link: '/guides/plugins/' }
          ]
        },
        {
          text: 'Section Reference',
          items: [
            { text: 'Overview', link: '/guides/reference/sections/' },
            { text: 'META', link: '/guides/reference/sections/meta' },
            { text: 'ADDRESS', link: '/guides/reference/sections/address' },
            { text: 'ENDPOINT', link: '/guides/reference/sections/endpoint' },
            { text: 'REQUEST', link: '/guides/reference/sections/request' },
            { text: 'RESPONSE', link: '/guides/reference/sections/response' },
            { text: 'ERROR', link: '/guides/reference/sections/error' },
            { text: 'ASSERTS', link: '/guides/reference/sections/asserts' },
            { text: 'EXTRACT', link: '/guides/reference/sections/extract' },
            { text: 'REQUEST_HEADERS', link: '/guides/reference/sections/request-headers' },
            { text: 'TLS', link: '/guides/reference/sections/tls' },
            { text: 'PROTO', link: '/guides/reference/sections/proto' },
            { text: 'OPTIONS', link: '/guides/reference/sections/options' },
            { text: 'BENCH', link: '/guides/reference/sections/bench' },
            { text: 'Attributes', link: '/guides/reference/sections/attributes' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/gripmock/grpctestify-rust' },
    ],

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright © 2025 gRPC Testify Contributors'
    },

    search: {
      provider: 'local',
      options: {
        locales: {
          root: {
            translations: {
              button: {
                buttonText: 'Search documentation...',
                buttonAriaLabel: 'Search documentation'
              },
              modal: {
                noResultsText: 'No results for',
                resetButtonTitle: 'Clear search',
                footer: {
                  selectText: 'to select',
                  navigateText: 'to navigate',
                  closeText: 'to close'
                }
              }
            }
          }
        }
      }
    },

    editLink: {
      pattern: 'https://github.com/gripmock/grpctestify-rust/edit/main/docs/:path',
      text: 'Edit this page on GitHub'
    },

    lastUpdated: {
      text: 'Last updated',
      formatOptions: {
        dateStyle: 'full',
        timeStyle: 'medium'
      }
    },

    docFooter: {
      prev: 'Previous page',
      next: 'Next page'
    },

    outline: {
      level: [2, 3],
      label: 'On this page'
    },

    aside: true,

    main: {
      padding: 'var(--vp-layout-top-height, 0px) 0 0 0'
    }
  },

  markdown: {
    theme: {
      light: 'github-light',
      dark: 'github-dark'
    },
    lineNumbers: true,
    config: (md) => {
      const defaultFence = md.renderer.rules.fence
      md.renderer.rules.fence = (tokens, idx, options, env, slf) => {
        const token = tokens[idx]
        if (token && typeof token.info === 'string') {
          const info = token.info.trim()
          if (info.startsWith('gctf')) {
            token.info = info.replace(/^gctf\b/, 'php')
          }
        }
        return defaultFence
          ? defaultFence(tokens, idx, options, env, slf)
          : slf.renderToken(tokens, idx, options)
      }
    }
  },

  vite: {
    css: {
      preprocessorOptions: {
        scss: {
          additionalData: `@import "./styles/variables.scss";`
        }
      }
    }
  }
})
