import { defineConfig } from 'vitest/config';

export default defineConfig({ test: { include: ['apps/desktop/ui/**/*.test.{ts,tsx}'], environment: 'jsdom', restoreMocks: true } });
