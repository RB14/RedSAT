// Syntax highlighting for RedSAT code blocks (```rsat) in the guide.
//
// mdBook highlights code with highlight.js, which does not know RedSAT, so this script
// registers an `rsat` language and highlights the blocks again. It runs after mdBook's own
// `book.js` (it is listed in `additional-js` in book.toml).
//
// The word lists below mirror the tables in `crates/redsat-lang/src/syntax.rs`, and carry the
// same names. `crates/redsat-lang/tests/grammar.rs` checks that they match exactly, so a new
// keyword or function cannot be forgotten here.

(function () {
  'use strict';

  const KEYWORDS = `
    var fn return require solve print if else for in while enum struct import
    and or not xor implies iff true false
  `;
  const CONTEXTUAL_KEYWORDS = `all unique limit minimize maximize bound`;
  const BUILTIN_TYPES = `bool int uint char string`;
  const BUILTIN_FUNCTIONS = `
    abs min max len sum count all any all_different reverse ord chr
  `;
  const PRELUDE_FUNCTIONS = `
    sign clamp divides is_even is_odd gcd digits digit_sum bit popcount
    exactly at_least at_most count_of
    product all_equal increasing strictly_increasing decreasing strictly_decreasing
    all_different_except index_of lex_less lex_less_eq sorted is_permutation
    is_digit is_lower is_upper is_letter to_upper to_lower digit_value
    starts_with ends_with count_char
  `;

  const words = (list) => list.trim().split(/\s+/);
  const LITERALS = ['true', 'false'];

  function rsat(hljs) {
    const keywords = {
      keyword: words(KEYWORDS).filter((w) => !LITERALS.includes(w)).join(' '),
      literal: LITERALS.join(' '),
      type: words(BUILTIN_TYPES).join(' '),
    };
    const NUMBER = {
      className: 'number',
      begin: /\b(?:0x[0-9A-Fa-f_]+|0b[01_]+|[0-9][0-9_]*)\b/,
      relevance: 0,
    };
    const CHAR = {
      className: 'string',
      begin: /'(?:\\.|[^\\'\n])'/,
    };
    // A built-in or standard library function, when it is called.
    const FUNCTIONS = {
      className: 'built_in',
      begin: new RegExp(
        '\\b(?:' + words(BUILTIN_FUNCTIONS).concat(words(PRELUDE_FUNCTIONS)).join('|') +
          ')(?=\\s*\\()',
      ),
    };
    // `solve all` and `solve unique`: `all` and `unique` are keywords only there.
    const SOLVE_MODE = {
      className: 'keyword',
      begin: /\bsolve\s+(?:all|unique)\b/,
    };
    // `limit`, `minimize`, `maximize` and `bound` are keywords when an expression follows,
    // and ordinary names otherwise (`var limit = 30`, `x <= limit`).
    const MODIFIER = {
      className: 'keyword',
      begin: /\b(?:limit|minimize|maximize|bound)\b(?!\s*(?:[=:,;)\]}.+\-*\/%<>]|$))/,
    };
    const HOLE = { className: 'literal', begin: /\?/ };
    const SUBST = {
      className: 'subst',
      begin: /\{/,
      end: /\}/,
      keywords: keywords,
      contains: [NUMBER, CHAR, FUNCTIONS],
    };
    const STRING = {
      className: 'string',
      begin: /"/,
      end: /"/,
      illegal: /\n/,
      contains: [hljs.BACKSLASH_ESCAPE, SUBST],
    };
    const DECLARATION = {
      beginKeywords: 'fn enum struct',
      end: /[({]/,
      excludeEnd: true,
      contains: [hljs.UNDERSCORE_TITLE_MODE],
    };
    return {
      name: 'RedSAT',
      aliases: ['redsat'],
      keywords: keywords,
      contains: [
        hljs.C_LINE_COMMENT_MODE,
        hljs.C_BLOCK_COMMENT_MODE,
        STRING,
        CHAR,
        NUMBER,
        SOLVE_MODE,
        MODIFIER,
        DECLARATION,
        FUNCTIONS,
        HOLE,
      ],
    };
  }

  if (typeof hljs === 'undefined') {
    return;
  }
  hljs.registerLanguage('rsat', rsat);

  const highlight = hljs.highlightElement || hljs.highlightBlock;
  document
    .querySelectorAll('code.language-rsat, code.language-redsat')
    .forEach(function (block) {
      // highlight.js 11 refuses to highlight a block twice without this.
      delete block.dataset.highlighted;
      highlight.call(hljs, block);
      block.classList.add('hljs');

      // `rsat,file=NAME` blocks are files that other examples import: show the name.
      const file = Array.from(block.classList).find((c) => c.startsWith('file='));
      const pre = block.parentElement;
      if (file && pre && !pre.querySelector('.rsat-file')) {
        const caption = document.createElement('div');
        caption.className = 'rsat-file';
        caption.textContent = file.slice('file='.length);
        pre.insertBefore(caption, block);
      }
    });
})();
