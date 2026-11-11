import js from '@eslint/js';
import ts from 'typescript-eslint';
export default ts.config({ ignores: ['**/dist/**', 'playwright-report/**', 'test-results/**'] }, js.configs.recommended, ...ts.configs.recommended, { files: ['**/*.cjs'], rules: { '@typescript-eslint/no-require-imports': 'off' }, languageOptions: { globals: { require: 'readonly', __dirname: 'readonly' } } }, { files: ['**/*.mjs', '**/*.cjs'], languageOptions: { globals: { process: 'readonly', console: 'readonly', Buffer: 'readonly', URL: 'readonly' } } });
