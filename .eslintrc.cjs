module.exports = {
  root: true,
  env: {
    browser: true,
    es2021: true,
  },
  parser: '@typescript-eslint/parser',
  parserOptions: {
    ecmaVersion: 'latest',
    sourceType: 'module',
    ecmaFeatures: {
      jsx: true,
    },
  },
  plugins: ['@typescript-eslint', 'react', 'react-hooks', 'react-refresh', 'jsx-a11y', 'import', 'i18next'],
  extends: [
    'eslint:recommended',
    'plugin:@typescript-eslint/recommended',
    'plugin:react/recommended',
    'plugin:react-hooks/recommended',
    'plugin:jsx-a11y/recommended',
    'prettier',
  ],
  settings: {
    react: {
      version: 'detect',
    },
  },
  rules: {
    // File & function size limits
    'max-lines': ['warn', { max: 500, skipBlankLines: true, skipComments: true }],
    'max-lines-per-function': ['warn', { max: 160, skipBlankLines: true, skipComments: true }],
    complexity: ['warn', { max: 20 }],

    // TypeScript
    '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
    '@typescript-eslint/consistent-type-imports': ['warn', { prefer: 'type-imports' }],
    '@typescript-eslint/no-explicit-any': 'warn',
    '@typescript-eslint/no-non-null-assertion': 'warn',
    '@typescript-eslint/prefer-nullish-coalescing': 'off', // needs type-checking parser (too slow)
    '@typescript-eslint/prefer-optional-chain': 'off', // needs type-checking parser

    // Import ordering & hygiene
    'import/order': ['warn', {
      groups: ['builtin', 'external', 'internal', 'parent', 'sibling', 'index'],
      'newlines-between': 'always',
      alphabetize: { order: 'asc', caseInsensitive: true },
    }],
    'import/no-duplicates': 'error',

    // React: arrow function components only
    'react/function-component-definition': ['error', {
      namedComponents: 'arrow-function',
      unnamedComponents: 'arrow-function',
    }],
    'react/react-in-jsx-scope': 'off',
    'react/prop-types': 'off',
    'react/jsx-no-leaked-render': ['warn', { validStrategies: ['ternary', 'coerce'] }],
    'react/self-closing-comp': ['warn', { component: true, html: true }],
    'react/jsx-curly-brace-presence': ['warn', { props: 'never', children: 'never' }],
    'react/jsx-boolean-value': ['warn', 'never'],
    'react/jsx-no-useless-fragment': 'warn',
    'react/hook-use-state': 'warn',
    'react/no-array-index-key': 'warn',
    'react/no-unstable-nested-components': 'warn',

    // React Refresh
    'react-refresh/only-export-components': ['warn', { allowConstantExport: true, allowExportNames: ['buttonVariants'] }],

    // Accessibility — warn for desktop app (not a public website)
    'jsx-a11y/click-events-have-key-events': 'warn',
    'jsx-a11y/no-static-element-interactions': 'warn',
    'jsx-a11y/label-has-associated-control': 'warn',
    'jsx-a11y/no-noninteractive-element-interactions': 'warn',

    // React naming & perf
    'react/jsx-handler-names': ['warn', {
      eventHandlerPrefix: 'handle',
      eventHandlerPropPrefix: 'on',
    }],

    // i18n — enforce translated strings (warn during migration, switch to error later)
    'i18next/no-literal-string': ['warn', {
      markupOnly: true,
      ignoreAttribute: [
        'className', 'style', 'type', 'key', 'id', 'name', 'htmlFor', 'role',
        'aria-label', 'aria-labelledby', 'aria-describedby',
        'data-testid', 'data-state', 'tabIndex', 'href', 'src', 'alt',
        'target', 'rel', 'method', 'action', 'value', 'defaultValue',
        'autoComplete', 'inputMode', 'pattern', 'accept',
      ],
      ignore: [
        // Patterns to ignore (regex)
        '^[A-Z_]+$',     // Constants like API_KEY
        '^\\d+$',         // Pure numbers
        '^[\\s·|/→←•—]+$', // Symbols & whitespace
        '^https?://',      // URLs
        '^\\.',            // File extensions
      ],
      ignoreCallee: [
        'console.log', 'console.warn', 'console.error', 'console.info',
        'require', 'import',
      ],
    }],

    // Code quality
    'prefer-const': 'error',
    'no-var': 'error',
    'eqeqeq': ['error', 'always', { null: 'ignore' }],
    'no-console': ['warn', { allow: ['warn', 'error'] }],
    'no-nested-ternary': 'warn',
    'no-unneeded-ternary': 'error',
    'no-else-return': 'warn',
    'prefer-template': 'warn',
    'object-shorthand': ['warn', 'always'],
    'prefer-destructuring': ['warn', {
      VariableDeclarator: { array: false, object: true },
      AssignmentExpression: { array: false, object: false },
    }],
    'prefer-arrow-callback': 'warn',
    'no-shadow': 'off',
    '@typescript-eslint/no-shadow': 'warn',
    'no-param-reassign': ['warn', { props: false }],
    'no-return-await': 'warn',
    'curly': ['warn', 'multi-line'],
    'no-magic-numbers': 'off',
    '@typescript-eslint/no-magic-numbers': ['warn', {
      ignore: [-1, 0, 1, 2, 3, 4, 5, 10, 16, 24, 60, 100, 1000, 1024],
      ignoreArrayIndexes: true,
      ignoreDefaultValues: true,
      ignoreEnums: true,
      ignoreReadonlyClassProperties: true,
      ignoreTypeIndexes: true,
      enforceConst: true,
    }],

  },
};
