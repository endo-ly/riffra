/** @type {import('stylelint').Config} */
export default {
  ignoreFiles: ['**/dist/**', '**/node_modules/**'],
  rules: {
    // Cross-panel layering must go through the scale in styles/tokens.css.
    'declaration-property-value-allowed-list': {
      'z-index': ['/^var\\(--z-/'],
    },
  },
  overrides: [
    {
      // Canvas modules own their internal stacking (grid lines under notes,
      // sticky headers over rows) and keep raw values, confined by the
      // isolation walls on their region roots.
      files: [
        'apps/desktop/src/features/arrange/WorkspaceArrange.module.css',
        'apps/desktop/src/features/arrange/midi-editor/MidiEditorPanel.module.css',
        'apps/desktop/src/features/arrange/play-surface/MusicalTypingKeyboard.module.css',
        'apps/desktop/src/features/arrange/inspector/Inspector.module.css',
        'apps/desktop/src/features/transport/TransportControls.module.css',
      ],
      rules: {
        'declaration-property-value-allowed-list': null,
      },
    },
  ],
};
