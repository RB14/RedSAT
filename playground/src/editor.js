// The code editor: CodeMirror 6 with RedSAT highlighting, live diagnostics from the compiler,
// completion, and hover tooltips showing docs and each unknown's inferred domain.

import { basicSetup } from 'codemirror';
import { indentWithTab } from '@codemirror/commands';
import { HighlightStyle, indentUnit, syntaxHighlighting } from '@codemirror/language';
import { linter, lintGutter, setDiagnostics } from '@codemirror/lint';
import { EditorState } from '@codemirror/state';
import { EditorView, hoverTooltip, keymap } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';

import { completions, rsatLanguage } from './rsat-language.js';

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
  '&': { height: '100%', fontSize: '14px', backgroundColor: 'var(--surface)', color: 'var(--text)' },
  '.cm-scroller': { fontFamily: 'var(--mono)', lineHeight: '1.55', fontVariantLigatures: 'none' },
  '.cm-content': { caretColor: 'var(--accent)', padding: '10px 0' },
  '.cm-cursor': { borderLeftColor: 'var(--accent)', borderLeftWidth: '2px' },
  '.cm-gutters': { backgroundColor: 'var(--surface)', color: 'var(--muted)', border: 'none' },
  '.cm-activeLine': { backgroundColor: 'var(--active-line)' },
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
  },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': { backgroundColor: 'var(--accent)', color: 'white' },
  '.cm-completionInfo': { maxWidth: '420px', whiteSpace: 'pre-wrap', fontFamily: 'var(--mono)', fontSize: '12px' },
  '.cm-diagnostic': { fontFamily: 'var(--mono)', fontSize: '12.5px', whiteSpace: 'pre-wrap' },
  '.cm-panels': { backgroundColor: 'var(--raised)', color: 'var(--text)' },
});

/** Hover: docs for keywords and functions, and the inferred domain of a declared unknown. */
function hover(words, latest) {
  return hoverTooltip((view, pos) => {
    const line = view.state.doc.lineAt(pos);
    const column = pos - line.from;
    let start = column;
    let end = column;
    while (start > 0 && /\w/.test(line.text[start - 1])) start--;
    while (end < line.text.length && /\w/.test(line.text[end])) end++;
    if (start === end) return null;
    const word = line.text.slice(start, end);
    const domain = latest.domains.find((d) => d.name === word || d.name.split('.')[0] === word);
    const doc = words.docs.get(word);
    if (!domain && !doc) return null;
    return {
      pos: line.from + start,
      end: line.from + end,
      above: true,
      create() {
        const dom = document.createElement('div');
        dom.className = 'hover';
        if (domain) {
          const head = document.createElement('div');
          head.className = 'hover-head';
          head.textContent = `${domain.name}: ${domain.domain}`;
          const body = document.createElement('div');
          const unknowns = domain.unknowns > 1 ? `${domain.unknowns} unknowns, ` : '';
          body.textContent = `inferred domain · ${unknowns}${domain.width} bit${domain.width === 1 ? '' : 's'} each`;
          dom.append(head, body);
        } else {
          const head = document.createElement('div');
          head.className = 'hover-head';
          head.textContent = doc.doc.split('\n')[0];
          dom.append(head);
          const rest = doc.doc.split('\n').slice(1).join(' ');
          if (rest) dom.append(Object.assign(document.createElement('div'), { textContent: rest }));
          dom.append(Object.assign(document.createElement('div'), { className: 'hover-kind', textContent: doc.kind }));
        }
        return { dom };
      },
    };
  });
}

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
 * @param {string} options.doc
 * @param {(source: string) => Promise<object>} options.analyze   diagnostics and domains
 * @param {() => void} options.onRun
 * @param {(source: string) => void} options.onChange
 */
export function createEditor({ parent, words, doc, analyze, onRun, onChange }) {
  const latest = { domains: [] };
  const language = rsatLanguage(words);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        basicSetup,
        language,
        language.data.of({ autocomplete: completions(words) }),
        syntaxHighlighting(highlightStyle),
        indentUnit.of('    '),
        theme,
        EditorView.lineWrapping,
        keymap.of([
          { key: 'Mod-Enter', run: () => (onRun(), true), preventDefault: true },
          { key: 'Shift-Enter', run: () => (onRun(), true), preventDefault: true },
          indentWithTab,
        ]),
        lintGutter(),
        linter(
          async (view) => {
            const result = await analyze(view.state.doc.toString());
            if (!result) return [];
            latest.domains = result.domains;
            return toLint(result.diagnostics, view.state.doc.length);
          },
          { delay: 350 },
        ),
        hover(words, latest),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onChange(update.state.doc.toString());
        }),
      ],
    }),
  });

  return {
    view,
    get text() {
      return view.state.doc.toString();
    },
    set text(text) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, selection: { anchor: 0 } });
      view.scrollDOM.scrollTop = 0;
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
}
