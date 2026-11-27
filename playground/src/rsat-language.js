// RedSAT syntax for CodeMirror: highlighting, indentation, completion and hover docs.
//
// Everything is built from the vocabulary the compiler itself reports (see `vocabulary()` in
// crates/redsat-wasm), so the editor never disagrees with the language.

import { StreamLanguage } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

const IDENT = /^[A-Za-z_][A-Za-z0-9_]*/;
const OPERATOR = /^(?:\.\.=|\.\.|==|!=|<=|>=|<<|>>|->|[-+*/%^&|<>=!])/;
const OPENERS = '([{';
const CLOSERS = ')]}';

/** Lookup tables derived from the compiler's vocabulary. */
export function lexicon(vocabulary) {
  const docs = new Map();
  for (const { name, doc } of [...vocabulary.keywords, ...vocabulary.contextual]) {
    if (doc) docs.set(name, { kind: 'keyword', doc });
  }
  for (const { name, doc } of vocabulary.builtins) docs.set(name, { kind: 'built-in', doc });
  for (const { name, doc } of vocabulary.prelude) docs.set(name, { kind: 'standard library', doc });
  return {
    keywords: new Set(vocabulary.keywords.map((k) => k.name).filter((k) => k !== 'true' && k !== 'false')),
    contextual: new Set(vocabulary.contextual.map((k) => k.name)),
    types: new Set(vocabulary.types),
    functions: new Set([...vocabulary.builtins.map((f) => f.name), ...vocabulary.prelude.map((f) => f.name)]),
    docs,
    vocabulary,
  };
}

function tokenizer(words) {
  return {
    name: 'rsat',
    startState: () => ({ inString: false, interpolation: 0, nesting: 0, afterFn: false }),
    copyState: (state) => ({ ...state }),
    token(stream, state) {
      if (stream.sol() && state.interpolation === 0) state.inString = false; // strings are one line
      // Inside a string literal, outside any {interpolation}.
      if (state.inString && state.interpolation === 0) {
        if (stream.eat('{')) {
          state.interpolation = 1;
          return 'interpolation';
        }
        while (!stream.eol()) {
          const ch = stream.peek();
          if (ch === '{') break;
          stream.next();
          if (ch === '\\') stream.next();
          else if (ch === '"') {
            state.inString = false;
            break;
          }
        }
        return 'string';
      }
      if (stream.eatSpace()) return null;
      if (stream.match('//')) {
        stream.skipToEnd();
        return 'comment';
      }
      const ch = stream.peek();
      if (state.interpolation > 0 && (ch === '{' || ch === '}')) {
        stream.next();
        state.interpolation += ch === '{' ? 1 : -1;
        return state.interpolation === 0 ? 'interpolation' : 'bracket';
      }
      if (ch === '"') {
        if (state.interpolation > 0) {
          stream.match(/^"(?:[^"\\]|\\.)*"?/); // a string inside an interpolation
          return 'string';
        }
        stream.next();
        state.inString = true;
        return 'string';
      }
      if (ch === "'") {
        if (stream.match(/^'(?:\\.[^']*|[^'\\])'/)) return 'char';
        stream.next();
        return null;
      }
      if (stream.match(/^\d+/)) return 'number';
      if (stream.match(IDENT)) {
        const word = stream.current();
        const afterFn = state.afterFn;
        state.afterFn = word === 'fn';
        if (word === 'true' || word === 'false') return 'bool';
        if (words.keywords.has(word)) return 'keyword';
        if (afterFn) return 'definition';
        if (/^\s*\(/.test(stream.string.slice(stream.pos))) {
          return words.functions.has(word) ? 'builtin' : 'function';
        }
        if (words.types.has(word)) return 'type';
        if (words.contextual.has(word)) return 'contextual';
        if (/^[A-Z]/.test(word)) return 'type';
        return 'variable';
      }
      if (stream.eat('?')) return 'hole';
      if (stream.match(OPERATOR)) return 'operator';
      stream.next();
      if (state.interpolation === 0) {
        if (OPENERS.includes(ch)) state.nesting++;
        if (CLOSERS.includes(ch)) state.nesting = Math.max(0, state.nesting - 1);
      }
      return OPENERS.includes(ch) || CLOSERS.includes(ch) ? 'bracket' : 'punctuation';
    },
    indent(state, textAfter, context) {
      const closing = CLOSERS.includes(textAfter.trimStart().charAt(0)) ? 1 : 0;
      return Math.max(0, state.nesting - closing) * context.unit;
    },
    languageData: {
      commentTokens: { line: '//' },
      closeBrackets: { brackets: ['(', '[', '{', '"', "'"] },
      indentOnInput: /^\s*[}\])]$/,
    },
    tokenTable: {
      keyword: t.keyword,
      contextual: t.modifier,
      type: t.typeName,
      builtin: t.standard(t.function(t.variableName)),
      function: t.function(t.variableName),
      definition: t.definition(t.function(t.variableName)),
      variable: t.variableName,
      number: t.number,
      string: t.string,
      char: t.character,
      bool: t.bool,
      comment: t.lineComment,
      operator: t.operator,
      hole: t.special(t.variableName),
      interpolation: t.special(t.brace),
      bracket: t.bracket,
      punctuation: t.punctuation,
    },
  };
}

/** The RedSAT language for CodeMirror, given the compiler's vocabulary. */
export function rsatLanguage(words) {
  return StreamLanguage.define(tokenizer(words));
}

/** A completion source: keywords, types, functions (with docs) and the program's own names. */
export function completions(words) {
  const fixed = [
    ...[...words.keywords, ...words.contextual].map((label) => ({ label, type: 'keyword', info: words.docs.get(label)?.doc })),
    ...[...words.types].map((label) => ({ label, type: 'type' })),
    ...words.vocabulary.builtins.map(({ name, doc }) => ({ label: name, type: 'function', detail: 'built-in', info: doc })),
    ...words.vocabulary.prelude.map(({ name, doc }) => ({
      label: name,
      type: 'function',
      detail: 'std',
      info: doc,
    })),
  ];
  const known = new Set(fixed.map((option) => option.label));
  return (context) => {
    const word = context.matchBefore(/[A-Za-z_][A-Za-z0-9_]*/);
    if (!word || (word.from === word.to && !context.explicit)) return null;
    const line = context.state.doc.lineAt(context.pos);
    if (line.text.slice(0, context.pos - line.from).includes('//')) return null; // in a comment
    const own = new Set(context.state.doc.toString().match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []);
    own.delete(word.text);
    const names = [...own].filter((name) => !known.has(name)).map((label) => ({ label, type: 'variable' }));
    return { from: word.from, options: [...fixed, ...names], validFor: /^[A-Za-z_][A-Za-z0-9_]*$/ };
  };
}
