// The code editor: CodeMirror 6 with RedSAT highlighting, live diagnostics from the compiler,
// completion with snippets, signature help, hover (docs, types and inferred domains) and
// go-to-definition (Ctrl/Cmd+click or F12).

import { basicSetup } from 'codemirror';
import { indentWithTab } from '@codemirror/commands';
import { HighlightStyle, indentUnit, syntaxHighlighting } from '@codemirror/language';
import { linter, lintGutter, setDiagnostics } from '@codemirror/lint';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';

import { clickToDefinition, completionSource, definitionAt, hover, signatureHelp } from './intelligence.js';
import { rsatLanguage } from './rsat-language.js';

const highlightStyle = HighlightStyle.define([
  { tag: t.keyword, color: 'var(--syn-keyword)', fontWeight: '600' },
  { tag: t.modifier, color: 'var(--syn-keyword)' },
  { tag: [t.typeName], color: 'var(--syn-type)' },
  { tag: [t.standard(t.function(t.variableName))], color: 'var(--syn-builtin)' },
  { tag: [t.function(t.variableName), t.definition(t.function(t.variableName))], color: 'var(--syn-function)' },
  { tag: [t.number, t.bool], color: 'var(--syn-number)' },
  { tag: [t.string, t.character], color: 'var(--syn-string)' },
  { tag: t.special(t.brace), color: 'var(--syn-interpolation)', fontWeight: '600' },
  { tag: t.special(t.variableName), color: 'var(--accent)', fontWeight: '700' },
  { tag: t.lineComment, color: 'var(--syn-comment)', fontStyle: 'italic' },
  { tag: t.operator, color: 'var(--syn-operator)' },
]);

const theme = EditorView.theme({
  '&': { height: '100%', fontSize: 'var(--editor-font-size)', backgroundColor: 'var(--surface)', color: 'var(--text)' },
  '.cm-scroller': { fontFamily: 'var(--mono)', lineHeight: '1.55', fontVariantLigatures: 'none' },
  '.cm-content': { caretColor: 'var(--accent)', padding: '10px 0' },
  '.cm-cursor': { borderLeftColor: 'var(--accent)', borderLeftWidth: '2px' },
  '.cm-gutters': { backgroundColor: 'var(--surface)', color: 'var(--muted)', border: 'none' },
  // Translucent: the selection is drawn behind the text and must show through on this line.
  '.cm-activeLine': { backgroundColor: 'var(--editor-active-line)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--active-line)', color: 'var(--text)' },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': {
    backgroundColor: 'var(--selection) !important',
  },
  '.cm-matchingBracket': { backgroundColor: 'var(--selection)', outline: 'none' },
  '.cm-tooltip': {
    backgroundColor: 'var(--raised)',
    color: 'var(--text)',
    border: '1px solid var(--border)',
    borderRadius: '8px',
    boxShadow: 'var(--shadow)',
    overflow: 'hidden',
    maxWidth: 'min(480px, 92vw)',
  },
  '.cm-tooltip-autocomplete > ul': { fontFamily: 'var(--mono)', maxHeight: '16em' },
  '.cm-tooltip-autocomplete > ul > li': { padding: '2px 8px !important', lineHeight: '1.6' },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': { backgroundColor: 'var(--accent-fill)', color: 'white' },
  '.cm-completionDetail': { color: 'var(--muted)', fontStyle: 'normal', marginLeft: '1em' },
  '.cm-tooltip-autocomplete > ul > li[aria-selected] .cm-completionDetail': { color: 'rgba(255,255,255,0.8)' },
  '.cm-completionInfo': { maxWidth: '420px', whiteSpace: 'pre-wrap', fontFamily: 'var(--mono)', fontSize: '12px', padding: '6px 10px' },
  '.cm-diagnostic': { fontFamily: 'var(--mono)', fontSize: '12.5px', whiteSpace: 'pre-wrap' },
  '.cm-panels': { backgroundColor: 'var(--raised)', color: 'var(--text)' },
  '.cm-snippetField': { backgroundColor: 'var(--selection)' },
});

function toLint(diagnostics, length) {
  return diagnostics.map((d) => {
    const from = Math.min(d.from, length);
    const to = Math.min(Math.max(d.to, d.from), length);
    const notes = d.notes.length ? `\n${d.notes.join('\n')}` : '';
    const label = d.label ? ` (${d.label})` : '';
    return { from, to, severity: d.severity, source: 'redsat', message: `${d.message}${label}${notes}` };
  });
}

/**
 * Creates the editor.
 * @param {object} options
 * @param {HTMLElement} options.parent
 * @param {object} options.words      the lexicon (see rsat-language.js)
 * @param {(source: string) => Promise<object|null>} options.analyze   the compiler's analysis
 * @param {() => void} options.onRun
 * @param {(source: string) => void} options.onChange
 */
export function createEditor({ parent, words, analyze, onRun, onChange }) {
  // The latest analysis. Symbols, structs and enums survive analyses of text that does not
  // parse, so completion keeps working while you type.
  const analysis = { domains: [], symbols: [], structs: [], enums: [], functions: [] };
  const latest = () => analysis;
  const language = rsatLanguage(words);
  let view;
  const reveal = (from, to) => api.reveal(from, to);

  const goToDefinition = () => {
    const target = definitionAt(view.state, view.state.selection.main.head, analysis);
    if (target) reveal(target.from, target.to);
    return Boolean(target);
  };

  const extensions = [
        basicSetup,
        language,
        language.data.of({ autocomplete: completionSource(words, latest) }),
        syntaxHighlighting(highlightStyle),
        indentUnit.of('    '),
        theme,
        EditorView.contentAttributes.of({ autocapitalize: 'off', autocorrect: 'off', spellcheck: 'false' }),
        keymap.of([
          { key: 'Mod-Enter', run: () => (onRun(), true), preventDefault: true },
          { key: 'Shift-Enter', run: () => (onRun(), true), preventDefault: true },
          { key: 'F12', run: goToDefinition, preventDefault: true },
          indentWithTab,
        ]),
        lintGutter(),
        linter(
          async (view) => {
            const result = await analyze(view.state.doc.toString());
            if (!result) return [];
            analysis.domains = result.domains;
            if (result.symbols) {
              analysis.functions = result.functions;
              analysis.symbols = result.symbols;
              analysis.structs = result.structs;
              analysis.enums = result.enums;
            }
            return toLint(result.diagnostics, view.state.doc.length);
          },
          { delay: 300 },
        ),
        signatureHelp(words, latest),
        hover(words, latest),
        clickToDefinition(latest, reveal),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onChange(update.state.doc.toString());
        }),
  ];
  view = new EditorView({ parent, state: EditorState.create({ doc: '', extensions }) });

  const api = {
    view,
    get text() {
      return view.state.doc.toString();
    },
    /** Opens another document: fresh undo history, no `onChange`. */
    load(text) {
      analysis.domains = [];
      view.setState(EditorState.create({ doc: text, extensions }));
      view.scrollDOM.scrollTop = 0;
    },
    /** Inserts text at the cursor; `select` places the cursor `back` characters before its end. */
    insert(text, back = 0) {
      const { from, to } = view.state.selection.main;
      view.dispatch({
        changes: { from, to, insert: text },
        selection: { anchor: from + text.length - back },
        scrollIntoView: true,
        userEvent: 'input.type',
      });
    },
    /** Shows diagnostics from a run (they replace the live ones until the next edit). */
    showDiagnostics(diagnostics) {
      view.dispatch(setDiagnostics(view.state, toLint(diagnostics, view.state.doc.length)));
    },
    /** Selects a range and scrolls it into view. */
    reveal(from, to) {
      const length = view.state.doc.length;
      view.dispatch({
        selection: { anchor: Math.min(from, length), head: Math.min(to, length) },
        effects: EditorView.scrollIntoView(Math.min(from, length), { y: 'center' }),
      });
      view.focus();
    },
    focus() {
      view.focus();
    },
  };
  return api;
}
