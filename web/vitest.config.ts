import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  // `luvo/*` is the framework half of this app: the design system and the
  // primitives, which know nothing about `.gctf` files.
  resolve: {
    alias: { luvo: fileURLToPath(new URL('./luvo', import.meta.url)) },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./luvo/test/setup.ts'],
  },
});
