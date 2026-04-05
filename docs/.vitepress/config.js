import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'gRPC Testify',
  description: 'Automate gRPC testing with simple .gctf files',
  ignoreDeadLinks: false,

  base: '/grpctestify-rust/',
  
  head: [
    ['link', { rel: 'icon', href: '/favicon.ico' }],
    ['meta', { name: 'theme-color', content: '#667eea' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'gRPC Testify' }]
  ],

  themeConfig: {
    logo: {
      light: '/logo-light.svg',
      dark: '/logo-dark.svg'
    },

    siteTitle: 'gRPC Testify',

    nav: [
      { text: 'Home', link: '/' },
      { text: 'Install', link: '/guides/getting-started/installation' },
      { text: 'CLI', link: '/guides/reference/api/command-line' },
      { text: 'Generator', link: '/generator' }
    ],

    sidebar: {
      '/guides/': [
        {
          text: 'Start',
          items: [
            { text: 'Installation', link: '/guides/getting-started/installation' },
            { text: 'First Test', link: '/guides/getting-started/first-test' },
            { text: 'Basic Concepts', link: '/guides/getting-started/basic-concepts' },
            { text: 'Troubleshooting', link: '/guides/troubleshooting' }
          ]
        },
        {
          text: 'Testing Patterns',
          items: [
            { text: 'Testing Patterns', link: '/guides/testing-patterns/testing-patterns' },
            { text: 'Data Validation', link: '/guides/testing-patterns/data-validation' },
            { text: 'Error Testing', link: '/guides/testing-patterns/error-testing' },
            { text: 'Security Testing', link: '/guides/testing-patterns/security-testing' },
            { text: 'Performance Testing', link: '/guides/testing-patterns/performance-testing' },
            { text: 'Assertion Patterns', link: '/guides/testing-patterns/assertion-patterns' }
          ]
        },
        {
          text: 'Reference',
          items: [
            { text: 'Command Line', link: '/guides/reference/api/command-line' },
            { text: 'Test Files', link: '/guides/reference/api/test-files' },
            { text: 'Assertions', link: '/guides/reference/api/assertions' },
            { text: 'Report Formats', link: '/guides/reference/api/report-formats' },
            { text: 'Type Validation', link: '/guides/reference/api/type-validation' },
            { text: 'Plugin Development', link: '/guides/reference/api/plugin-development' },
            { text: 'Core gRPC Execution', link: '/guides/reference/api/grpc-default-test' }
          ]
        },
        {
          text: 'Plugins',
          items: [
            { text: 'Plugin System', link: '/guides/plugins/' },
            { text: 'Timing Assertions', link: '/guides/plugins/timing-assertions' },
            { text: 'State API', link: '/guides/plugins/development/state-api' }
          ]
        },
        {
          text: 'Example',
          items: [
            { text: 'Real-time Chat', link: '/guides/examples/basic/real-time-chat' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/gripmock/grpctestify-rust' },
    ],

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright © 2024 gRPC Testify Contributors'
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
