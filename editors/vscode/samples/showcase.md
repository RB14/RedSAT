# RedSAT in Markdown

A backtick fence tagged `rsat`:

```rsat
// Find the two prime factors of a semiprime.
var n = 1000036000099
var x, y
require x > 1 and y > 1 and x * y == n
solve x, y {
    print "{n} = {x} × {y}"
}
```

A tilde fence tagged `redsat`, with extra info-string attributes:

~~~redsat title="queens"
var queen: [int[0..8]; 8]
solve all queen limit 3
~~~

A longer backtick fence (4 backticks) containing a 3-backtick line, which must not close it:

````RSat
var s: string
```
require len(s) == 3
````

A non-RedSAT fence must stay isolated (no RedSAT scopes inside):

```python
var = solve(require)   # Python, not RedSAT
```

An untagged fence:

```
require x > 1
```

<!--
```rsat
var hidden = 1
```
-->

Text after the blocks is plain Markdown with `inline code`.
