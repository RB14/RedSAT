// Editor intelligence for RedSAT: completion, signature help, hover and go-to-definition.
//
// Built-in and standard-library knowledge comes from the compiler's vocabulary; knowledge of
// the program comes from the compiler's latest analysis (symbols with their types and scopes,
// structs, enums, inferred domains), with the program text as a fallback while it does not
// parse.

import { snippetCompletion } from '@codemirror/autocomplete';
import { StateField } from '@codemirror/state';
import { EditorView, hoverTooltip, showTooltip } from '@codemirror/view';

const IDENT = /[A-Za-z_][A-Za-z0-9_]*/;

/** Splits a parameter list at top-level commas. */
function splitParams(text) {
  const params = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i <= text.length; i++) {
    const ch = text[i];
    if (ch === '(' || ch === '[' || ch === '{') depth++;
    else if (ch === ')' || ch === ']' || ch === '}') depth--;
    else if ((ch === ',' && depth === 0) || i === text.length) {
      const param = text.slice(start, i).trim();
      if (param) params.push(param);
      start = i + 1;
    }
  }
  return params;
}

/** Signatures of the built-in and standard-library functions, by name. */
export function librarySignatures(vocabulary) {
  const signatures = new Map();
  for (const { name, doc } of vocabulary.builtins) {
    const colon = doc.indexOf(': ');
    const label = colon < 0 ? `${name}(…)` : doc.slice(0, colon);
    const call = new RegExp(`^${name}\\(([^)]*)\\)`).exec(label);
    signatures.set(name, {
      name,
      label,
      params: call ? splitParams(call[1]).filter((p) => p !== '…') : [],
      doc: colon < 0 ? doc : doc.slice(colon + 2),
      kind: 'built-in',
    });
  }
  for (const { name, doc } of vocabulary.prelude) {
    const [line, ...rest] = doc.split('\n');
    const match = /^fn\s+\w+\(([^)]*)\)/.exec(line.trim());
    signatures.set(name, {
      name,
      label: line.trim().replace(/^fn\s+/, ''),
      params: match ? splitParams(match[1]) : [],
      doc: rest.join(' '),
      kind: 'standard library',
    });
  }
  return signatures;
}

/** Signatures of the functions declared in `text`, by name (with where each is declared). */
export function programSignatures(text) {
  const signatures = new Map();
  const pattern = /\bfn\s+([A-Za-z_]\w*)\s*\(([^)]*)\)\s*(?:->\s*([^{\n]+?))?\s*\{/g;
  for (const match of text.matchAll(pattern)) {
    const [, name, params, result] = match;
    signatures.set(name, {
      name,
      label: `${name}(${params.trim()})${result ? ` -> ${result.trim()}` : ''}`,
      params: splitParams(params),
      doc: '',
      kind: 'your function',
      from: match.index + match[0].indexOf(name),
    });
  }
  return signatures;
}

/** The snippet that calls a function, with a placeholder per parameter. */
function callSnippet(signature) {
  const placeholders = signature.params.map((param) => `\${${param.split(':')[0].trim()}}`);
  return `${signature.name}(${placeholders.join(', ') || '${}'})`;
}

/** Statement templates: [label, template, description]. */
const SNIPPETS = [
  ['var', 'var ${name}: int[${0}..=${9}]', 'declare an unknown with a range'],
  ['var =', 'var ${name} = ${value}', 'define a known value'],
  ['require', 'require ${condition}', 'every solution must satisfy the condition'],
  ['solve', 'solve ${x}', 'find one solution'],
  ['solve all', 'solve all ${x} {\n\tprint ${x}\n}', 'find every solution, running the body for each'],
  ['solve unique', 'solve unique ${x}', 'find the solution and prove there is no other'],
  ['solve minimize', 'solve ${x} minimize ${cost}', 'find a solution with the smallest cost'],
  ['solve maximize', 'solve ${x} maximize ${value}', 'find a solution with the largest value'],
  ['fn', 'fn ${name}(${x}: int) -> ${int} {\n\treturn ${x}\n}', 'define a function'],
  ['for', 'for ${i} in ${0}..${n} {\n\t${}\n}', 'loop over a range'],
  ['for in', 'for ${x} in ${xs} {\n\t${}\n}', 'loop over an array or string'],
  ['if', 'if ${condition} {\n\t${}\n}', 'branch (works on unknowns too)'],
  ['if else', 'if ${condition} {\n\t${}\n} else {\n\t${}\n}', 'branch with an alternative'],
  ['while', 'while ${condition} bound ${100} {\n\t${}\n}', 'loop with an iteration bound'],
  ['struct', 'struct ${Name} { ${field}: ${int} }', 'define a struct'],
  ['enum', 'enum ${Name} { ${A}, ${B} }', 'define an enum'],
  ['print', 'print ${value}', 'print values'],
  ['import', 'import "${file}.rsat"', "use another file's functions, enums and structs"],
];

const SNIPPET_KEYWORDS = new Set(['var', 'require', 'solve', 'fn', 'for', 'if', 'while', 'struct', 'enum', 'print', 'import']);

/** Whether `pos` is inside a comment or (outside an interpolation) a string. */
function inCommentOrString(lineText, column) {
  let inString = false;
  let interpolation = 0;
  for (let i = 0; i < column; i++) {
    const ch = lineText[i];
    if (!inString && interpolation === 0 && ch === '/' && lineText[i + 1] === '/') return true;
    if (inString && interpolation === 0) {
      if (ch === '\\') i++;
      else if (ch === '"') inString = false;
      else if (ch === '{') interpolation = 1;
    } else if (interpolation > 0) {
      if (ch === '{') interpolation++;
      else if (ch === '}') interpolation--;
    } else if (ch === '"') {
      inString = true;
    } else if (ch === "'") {
      const end = lineText.indexOf("'", i + 2);
      if (end > 0 && end < column) i = end;
      else return true;
    }
  }
  return inString && interpolation === 0;
}

/** The symbol called `name` visible at `pos` (the innermost, latest declaration). */
function visibleSymbol(analysis, name, pos) {
  let best = null;
  for (const symbol of analysis?.symbols ?? []) {
    if (symbol.name !== name || pos < symbol.scopeFrom || pos > symbol.scopeTo) continue;
    if (!best || symbol.from > best.from) best = symbol;
  }
  return best ?? analysis?.symbols?.find((symbol) => symbol.name === name) ?? null;
}

/** Structs and enums declared in the text (always current, unlike the analysis). */
function declaredTypes(text) {
  const structs = [];
  for (const match of text.matchAll(/\bstruct\s+([A-Za-z_]\w*)\s*\{([^}]*)\}/g)) {
    const fields = splitParams(match[2].replace(/\n/g, ','))
      .map((field) => /^([A-Za-z_]\w*)\s*:\s*(.+)$/.exec(field))
      .filter(Boolean)
      .map(([, name, type]) => ({ name, type: type.trim() }));
    structs.push({ name: match[1], fields });
  }
  const enums = [];
  for (const match of text.matchAll(/\benum\s+([A-Za-z_]\w*)\s*\{([^}]*)\}/g)) {
    enums.push({ name: match[1], variants: match[2].split(/[\s,]+/).filter((v) => /^[A-Za-z_]\w*$/.test(v)) });
  }
  return { structs, enums };
}

/** The declared type of `name` in the text: `var a, name: T`, `name: T` (a parameter). */
function declaredTypeOf(text, name) {
  const declaration = new RegExp(`\\bvar\\s+(?:[A-Za-z_]\\w*\\s*,\\s*)*${name}\\b[^:=\\n]*:\\s*([^=\\n]+)|[(,]\\s*${name}\\s*:\\s*([^,)]+)`);
  const match = declaration.exec(text);
  return match ? (match[1] ?? match[2]).trim() : null;
}

/** The struct an expression of type `type` (e.g. `Point`, `[Point; 3]`) holds after `indexed` indexings. */
function structNamed(structs, type, indexed) {
  let inner = type.trim();
  for (let i = 0; i < indexed; i++) inner = inner.replace(/^\[\s*(.*?)\s*(?:;[^\]]*)?\]$/, '$1');
  return structs.find((s) => s.name === inner) ?? null;
}

/** Members after a dot: enum variants or struct fields. */
function memberOptions(analysis, text, before, pos) {
  const match = /([A-Za-z_]\w*)((?:\[[^\]]*\])*)\.(\w*)$/.exec(before);
  if (!match) return null;
  const [, base, indices, partial] = match;
  const from = pos - partial.length;
  const declared = declaredTypes(text);
  const structs = [...declared.structs, ...(analysis?.structs ?? [])];
  const enums = [...declared.enums, ...(analysis?.enums ?? [])];
  const enumInfo = enums.find((e) => e.name === base);
  if (enumInfo && !indices) {
    return { from, options: enumInfo.variants.map((variant, i) => ({ label: variant, type: 'enum', detail: enumInfo.name, boost: -i })) };
  }
  const type = visibleSymbol(analysis, base, pos)?.detail ?? declaredTypeOf(text, base);
  const struct = type && structNamed(structs, type, (indices.match(/\[/g) ?? []).length);
  if (struct) {
    return { from, options: struct.fields.map((field, i) => ({ label: field.name, type: 'property', detail: field.type, boost: -i })) };
  }
  return { from, options: [] };
}

/** The completion source. `latest()` returns the latest analysis with symbols. */
export function completionSource(words, latest) {
  const library = librarySignatures(words.vocabulary);
  const keywordOptions = [...words.keywords, ...words.contextual]
    .filter((label) => !SNIPPET_KEYWORDS.has(label))
    .map((label) => ({ label, type: 'keyword', info: words.docs.get(label)?.doc, boost: -1 }));
  const typeOptions = [...words.types].map((label) => ({ label, type: 'type' }));
  const snippetOptions = SNIPPETS.map(([label, template, detail]) =>
    snippetCompletion(template, { label, type: 'keyword', detail, info: words.docs.get(label.split(' ')[0])?.doc }),
  );
  const libraryOptions = [...library.values()].map((signature) =>
    snippetCompletion(callSnippet(signature), {
      label: signature.name,
      type: 'function',
      detail: signature.kind === 'built-in' ? 'built-in' : 'std',
      info: `${signature.label}\n${signature.doc}`,
      boost: -2,
    }),
  );

  return (context) => {
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    if (inCommentOrString(line.text, context.pos - line.from)) return null;
    const analysis = latest();
    const text = context.state.doc.toString();

    const members = memberOptions(analysis, text, before, context.pos);
    if (members) return { ...members, validFor: /^\w*$/ };

    const word = context.matchBefore(/[A-Za-z_][A-Za-z0-9_]*/);
    if (!word || (word.from === word.to && !context.explicit)) return null;
    if (/\d$/.test(before.slice(0, before.length - word.text.length))) return null; // e.g. `0..9`

    const own = programSignatures(text);
    const functionOptions = [...own.values()]
      .filter((signature) => signature.name !== word.text)
      .map((signature) =>
        snippetCompletion(callSnippet(signature), { label: signature.name, type: 'function', detail: signature.label.slice(signature.name.length) }),
      );
    const seen = new Set([...own.keys(), word.text]);
    const symbolOptions = [];
    for (const symbol of analysis?.symbols ?? []) {
      if (seen.has(symbol.name) || symbol.kind === 'function') continue;
      if (context.pos < symbol.scopeFrom || context.pos > symbol.scopeTo) continue;
      seen.add(symbol.name);
      const type = symbol.kind === 'enum' ? 'enum' : symbol.kind === 'struct' ? 'class' : 'variable';
      symbolOptions.push({ label: symbol.name, type, detail: symbol.detail, boost: 2 });
    }
    // Names in the text the analysis does not know yet (e.g. while the program does not parse).
    const known = new Set([...seen, ...words.keywords, ...words.contextual, ...words.types, ...library.keys()]);
    const textOptions = [...new Set(text.match(new RegExp(IDENT, 'g')) ?? [])]
      .filter((name) => !known.has(name))
      .map((label) => ({ label, type: 'text', boost: -3 }));

    const atStatementStart = /^\s*[A-Za-z_]*$/.test(before);
    return {
      from: word.from,
      options: [
        ...symbolOptions,
        ...functionOptions,
        ...libraryOptions,
        ...(atStatementStart ? snippetOptions : snippetOptions.filter((o) => o.label === 'if')),
        ...keywordOptions,
        ...typeOptions,
        ...textOptions,
      ],
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  };
}

// --- signature help -------------------------------------------------------------------------

/** The call around `pos`: the function's name and which argument the cursor is in. */
function callAt(text, pos) {
  let depth = 0;
  let argument = 0;
  for (let i = pos - 1; i >= Math.max(0, pos - 3000); i--) {
    const ch = text[i];
    if (ch === ')' || ch === ']' || ch === '}') depth++;
    else if (ch === '(' || ch === '[' || ch === '{') {
      if (depth > 0) {
        depth--;
        continue;
      }
      if (ch !== '(') return null;
      const name = /([A-Za-z_]\w*)\s*$/.exec(text.slice(Math.max(0, i - 64), i));
      return name ? { name: name[1], argument, open: i } : null;
    } else if (ch === ',' && depth === 0) argument++;
    else if (ch === ';') return null;
  }
  return null;
}

function signatureTooltip(state, library) {
  const selection = state.selection.main;
  if (!selection.empty) return null;
  const text = state.doc.toString();
  const call = callAt(text, selection.head);
  if (!call) return null;
  const signature = programSignatures(text).get(call.name) ?? library.get(call.name);
  if (!signature) return null;
  return {
    pos: call.open,
    above: true,
    strictSide: true,
    arrow: false,
    create() {
      const dom = document.createElement('div');
      dom.className = 'signature';
      const head = document.createElement('div');
      head.className = 'signature-head';
      const params = signature.params;
      const active = Math.min(call.argument, params.length - 1);
      const open = signature.label.indexOf('(');
      const close = signature.label.indexOf(')', open);
      if (open < 0 || close < 0 || !params.length) {
        head.textContent = signature.label;
      } else {
        head.append(signature.label.slice(0, open + 1));
        params.forEach((param, index) => {
          if (index > 0) head.append(', ');
          const span = document.createElement('span');
          span.textContent = param;
          if (index === active) span.className = 'active-param';
          head.append(span);
        });
        head.append(signature.label.slice(close));
      }
      dom.append(head);
      if (signature.doc) dom.append(Object.assign(document.createElement('div'), { className: 'signature-doc', textContent: signature.doc }));
      return { dom };
    },
  };
}

/** Shows the signature of the function being called, highlighting the current argument. */
export function signatureHelp(words) {
  const library = librarySignatures(words.vocabulary);
  return StateField.define({
    create: (state) => signatureTooltip(state, library),
    update: (value, transaction) =>
      transaction.docChanged || transaction.selection ? signatureTooltip(transaction.state, library) : value,
    provide: (field) => showTooltip.from(field),
  });
}

// --- hover and definitions ------------------------------------------------------------------

function wordAt(state, pos) {
  const line = state.doc.lineAt(pos);
  let start = pos - line.from;
  let end = start;
  while (start > 0 && /\w/.test(line.text[start - 1])) start--;
  while (end < line.text.length && /\w/.test(line.text[end])) end++;
  if (start === end || /^\d/.test(line.text.slice(start, end))) return null;
  return { text: line.text.slice(start, end), from: line.from + start, to: line.from + end };
}

/** Where the name at `pos` is declared, if known. */
export function definitionAt(state, pos, analysis) {
  const word = wordAt(state, pos);
  if (!word) return null;
  const own = programSignatures(state.doc.toString()).get(word.text);
  if (own) return { from: own.from, to: own.from + word.text.length };
  const symbol = visibleSymbol(analysis, word.text, pos);
  return symbol ? { from: symbol.from, to: symbol.to } : null;
}

function element(className, text) {
  return Object.assign(document.createElement('div'), { className, textContent: text });
}

/** Hover: inferred domains of unknowns, types of names, docs of keywords and functions. */
export function hover(words, latest) {
  const library = librarySignatures(words.vocabulary);
  return hoverTooltip((view, pos) => {
    const word = wordAt(view.state, pos);
    if (!word) return null;
    const analysis = latest();
    const domain = analysis?.domains?.find((d) => d.name === word.text || d.name.split(/[.[]/)[0] === word.text);
    const own = programSignatures(view.state.doc.toString()).get(word.text);
    const symbol = visibleSymbol(analysis, word.text, pos);
    const signature = library.get(word.text);
    const keyword = words.docs.get(word.text);
    const parts = [];
    if (domain) {
      parts.push(element('hover-head', `${domain.name}: ${domain.domain}`));
      const unknowns = domain.unknowns > 1 ? `${domain.unknowns} unknowns, ` : '';
      parts.push(element('', `inferred domain · ${unknowns}${domain.width} bit${domain.width === 1 ? '' : 's'} each`));
    } else if (own) {
      parts.push(element('hover-head', `fn ${own.label}`), element('hover-kind', 'your function · Ctrl+click to go to it'));
    } else if (symbol) {
      parts.push(element('hover-head', `${symbol.name}: ${symbol.detail}`), element('hover-kind', symbol.kind));
    } else if (signature) {
      parts.push(element('hover-head', signature.label));
      if (signature.doc) parts.push(element('', signature.doc));
      parts.push(element('hover-kind', signature.kind));
    } else if (keyword) {
      parts.push(element('', keyword.doc), element('hover-kind', 'keyword'));
    }
    if (!parts.length) return null;
    return {
      pos: word.from,
      end: word.to,
      above: true,
      create() {
        const dom = document.createElement('div');
        dom.className = 'hover';
        dom.append(...parts);
        return { dom };
      },
    };
  });
}

/** Ctrl/Cmd+click goes to a name's declaration. */
export function clickToDefinition(latest, reveal) {
  return EditorView.domEventHandlers({
    mousedown(event, view) {
      if (!(event.ctrlKey || event.metaKey)) return false;
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return false;
      const target = definitionAt(view.state, pos, latest());
      if (!target) return false;
      event.preventDefault();
      reveal(target.from, target.to);
      return true;
    },
  });
}
