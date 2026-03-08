import DefaultTheme from 'vitepress/theme'
import GctfGenerator from './components/GctfGenerator.vue'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('GctfGenerator', GctfGenerator)
  }
}
