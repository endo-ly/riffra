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
      /*
       * Canvas compositions (timeline lanes, piano keys, grid lines) own
       * internal stacking ladders; their region roots are isolated so the
       * raw values never compete across panels.
       */
      files: [
        'apps/desktop/src/features/arrange/WorkspaceArrange.module.css',
        'apps/desktop/src/features/arrange/midi-editor/MidiEditorPanel.module.css',
        'apps/desktop/src/features/arrange/play-surface/MusicalTypingKeyboard.module.css',
      ],
      rules: {
        'declaration-property-value-allowed-list': null,
      },
    },
  ],
};
