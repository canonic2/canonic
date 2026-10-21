import js from '@eslint/js';
import wc from 'eslint-plugin-wc';

const readonly = (names) => Object.fromEntries(names.split(' ').map((name) => [name, 'readonly']));
const browser = readonly(
  'window document location history parent navigator localStorage innerWidth innerHeight devicePixelRatio crypto fetch XMLHttpRequest WebSocket Event CustomEvent MessageEvent KeyboardEvent PointerEvent HTMLElement HTMLInputElement HTMLTextAreaElement HTMLSelectElement HTMLButtonElement HTMLIFrameElement Element Node ShadowRoot CSSStyleSheet ResizeObserver MutationObserver AbortController AbortSignal URL URLSearchParams Blob Image File FileReader DOMParser XMLSerializer requestAnimationFrame cancelAnimationFrame setTimeout clearTimeout setInterval clearInterval queueMicrotask structuredClone console performance customElements getComputedStyle atob btoa',
);
const node = readonly('process Buffer global setImmediate clearImmediate');
export default [
  { ignores: ['**/node_modules/**', 'ui/src/vendor/**'] },
  {
    ...js.configs.recommended,
    files: ['ui/src/**/*.js'],
    languageOptions: {
      globals: {
        ...browser,
        ...readonly(
          'Option CSS EventTarget addEventListener sessionStorage createImageBitmap Worker self DOMException',
        ),
      },
    },
  },
  { files: ['ui/src/**/*.test.js'], languageOptions: { globals: node } },
  {
    files: ['ui/src/**/*.js'],
    ignores: ['**/*.test.js'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: [
                'node:*',
                '**/server/**',
                '**/extension/**',
                '**/capture/**',
                'vscode',
                'electron',
                '**/*.ts',
              ],
              message:
                'Browser code uses native JavaScript modules and browser-safe ports, not host services or transformed sources.',
            },
          ],
        },
      ],
    },
  },
  {
    ...wc.configs['flat/recommended'],
    files: ['ui/src/components/**/element.js'],
    rules: {
      ...wc.configs['flat/recommended'].rules,
      'wc/no-closed-shadow-root': 'error',
      'wc/no-constructor-params': 'error',
      'wc/no-invalid-extends': 'error',
      'wc/no-typos': 'error',
    },
  },
  {
    files: ['ui/src/components/**/*.js'],
    ignores: ['**/*.test.js'],
    rules: {
      'no-restricted-globals': [
        'error',
        { name: 'fetch', message: 'Components emit intent; controllers own network I/O.' },
        'XMLHttpRequest',
        'WebSocket',
      ],
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: [
                'node:*',
                '**/server/**',
                '**/extension/**',
                '**/capture/**',
                '**/host/**',
                '**/bootstrap/**',
                '**/legacy/**',
                'vscode',
                'electron',
                '**/*.ts',
              ],
              message:
                'Presentation components depend on pure models and helpers; controllers and host adapters compose operations.',
            },
            {
              regex: '(^|/)modules/(?!canvas/presentation\\.js$)',
              message:
                'Components consume the pure presentation contract, not capability operations.',
            },
          ],
        },
      ],
    },
  },
];
