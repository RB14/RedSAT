// The playground's addresses, kept in the URL hash (GitHub Pages serves no other paths):
//
//   #/                      home
//   #/examples              the examples (on phones, their own page)
//   #/examples/two-numbers  an example, by its title (its file name, `bigger-of-two`, also works)
//   #/files/my-puzzle       one of your files, by name (they live in this browser only)
//   #code=…                 a shared program (the deflated source; see share.js)

/** A readable address segment: `Pell's equation` → `pells-equation`, `SEND + MORE = MONEY` →
 * `send-more-money`. */
export function slugOf(text) {
  return text
    .toLowerCase()
    .normalize('NFKD')
    .replace(/['’]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

/** The address of an example. */
export const exampleRoute = (example) => `#/examples/${slugOf(example.title)}`;

/** The address of one of your files. */
export const fileRoute = (name) => `#/files/${encodeURIComponent(name)}`;

/** The example an address segment names: its title's slug, its file name, or that with dashes. */
export function exampleBySlug(examples, slug) {
  return examples.find((example) => [slugOf(example.title), slugOf(example.name), example.name].includes(slug));
}

/** `{section, key}` of an address: `#/examples/sudoku` → `{section: 'examples', key: 'sudoku'}`;
 * the home page has no section. (Shared programs, `#code=…`, are decoded by share.js.) */
export function parseRoute(hash) {
  const [section = '', key = ''] = hash
    .replace(/^#\/?/, '')
    .split('/')
    .map((part) => {
      try {
        return decodeURIComponent(part);
      } catch {
        return part;
      }
    });
  return { section, key };
}
