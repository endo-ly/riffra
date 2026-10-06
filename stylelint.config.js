const fontSizeScale = ['/^var\\(--font-size-/', 'inherit'];

/** @type {import('stylelint').Config} */
export default {
  ignoreFiles: ['**/dist/**', '**/node_modules/**'],
  rules: {
    // Cross-panel layering and type sizes go through the scales in styles/tokens.css.
    'declaration-property-value-allowed-list': {
      'z-index': ['/^var\\(--z-/'],
      'font-size': fontSizeScale,
    },
    // Spacing uses the --space-* scale; only hairline (1px) offsets stay literal.
    'declaration-property-value-disallowed-list': {
      '/^(padding|margin|gap|row-gap|column-gap)/': ['/(^|[\\s(,-])([2-9]|\\d{2,})(\\.\\d+)?px/'],
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
        'declaration-property-value-allowed-list': { 'font-size': fontSizeScale },
      },
    },
  ],
};
