const js = require('@eslint/js');
const tseslint = require('typescript-eslint');
const i18next = require('eslint-plugin-i18next');

const tsFiles = ['**/*.{ts,tsx}'];

const jsRecommendedForTs = {
  ...js.configs.recommended,
  files: tsFiles,
};

const tsRecommended = tseslint.configs.recommended.map((config) =>
  config.files ? config : { ...config, files: tsFiles },
);

module.exports = [
  {
    // .claude/ holds agents' git worktrees, each a full copy of this tree. Flat config does not read .gitignore.
    ignores: ['dist/**', 'node_modules/**', 'src-tauri/target/**', 'src-tauri/gen/**', '.claude/**'],
  },
  jsRecommendedForTs,
  ...tsRecommended,
  {
    files: tsFiles,
    plugins: {
      i18next,
    },
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      parserOptions: {
        ecmaFeatures: {
          jsx: true,
        },
      },
    },
    // Every rule is an error: scripts/check-frontend.mjs counts errors against its baseline and
    // ignores warnings, so a warning is a rule nothing enforces.
    rules: {
      'no-unused-vars': 'off',
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
      'i18next/no-literal-string': [
        'error',
        {
          markupOnly: true,
          ignoreAttribute: [
            'className',
            'style',
            'data-tooltip',
            'variant',
            'size',
            'color',
            'weight',
            'fillOrigin',
            'id',
            'name',
            'type',
            'value',
            'label',
            'placeholder',
            'stroke',
            'fill',
            'viewBox',
          ],
        },
      ],
    },
  },
];
