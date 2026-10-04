import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/.pnpm-store/**',
      '.runtime/**',
      '.studio/**',
      'artifacts/**',
      'dist/**',
    ],
  },
  {
    files: ['**/*.{ts,cts}'],
    extends: [js.configs.recommended, ...tseslint.configs.recommendedTypeChecked],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    rules: {
      '@typescript-eslint/consistent-type-imports': ['error', { prefer: 'type-imports' }],
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/no-floating-promises': 'error',
    },
  },
  {
    files: ['**/*.{js,cjs}'],
    extends: [js.configs.recommended],
    languageOptions: {
      globals: {
        require: 'readonly',
        module: 'readonly',
        console: 'readonly',
        process: 'readonly',
        setTimeout: 'readonly',
        Buffer: 'readonly',
      },
    },
  },
  {
    files: ['test/integration/**/*.ts', 'test/**/*.test.ts', 'app/preload.cts'],
    // Electron's executeJavaScript/invoke and fixture JSON are untyped external APIs.
    rules: {
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-unsafe-return': 'off',
      '@typescript-eslint/no-unsafe-call': 'off',
    },
  },
  prettier,
);
