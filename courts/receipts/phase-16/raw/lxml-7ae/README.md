# lxml court 7ae — EXSLT XPath-context registration

`candidate_sha=eaffc17f` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (2eece63c / lxml-7ad)   93 unique failing ids
now (eaffc17f / lxml-7ae)        89 unique failing ids
fixed                             4
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

Fixed ids:

```
test_xpath_exslt_functions_date     lxml.etree.XPathEvalError: Unregistered function: date:year
test_xpath_exslt_functions_strings  lxml.etree.XPathEvalError: Unregistered function: str:align
test_exslt_math                     pi="3.141592653589793" (expected 3.14 / 3)
test_exslt_str                      ' B ' (expected '*B*')
```

## Root cause 1 — registration went to the wrong object

lxml enables EXSLT for a plain `XPath` evaluator by scanning the context's
namespace hash and calling the module entry point for each matching URI
(`xpath.pxi::_registerExsltFunctionsForNamespaces`):

```cython
if tree.xmlStrcmp(c_href, xslt.EXSLT_DATE_NAMESPACE) == 0:
    xslt.exsltDateXpathCtxtRegister(ctxt, c_prefix)
...
```

Upstream `exsltDateXpathCtxtRegister` (libexslt `date.c`) calls
`xmlXPathRegisterNs(ctxt, prefix, NS)` and then one
`xmlXPathRegisterFuncNS(ctxt, local, NS, fn)` per function — i.e. it registers
onto **the context it is handed**.

The candidate ignored both arguments and called `dates::register_all()`, which
writes to the **process-wide** registry. In the whole-archive facade layout
that registry is libexslt's private copy; nothing reads it, and in any case a
plain XPath context is never given the process-wide registry. Hence
`Unregistered function: date:year`.

The four entry points now register onto the passed context, under the module's
namespace URI, exactly like upstream. The per-module `FUNCTIONS` tables
(`dates`, `strings`, `sets`, `math`) are the single source of truth shared with
`register_all()`, so the two paths cannot drift.

Resolution path after the fix: `date:year` is not in the context map under its
raw name, so `resolve_function_qualified_name` maps the `date` prefix through
the registered namespace to `{http://exslt.org/dates-and-times}year`, which is
the key the registration installed.

## Root cause 2 — `str:align` used spaces, not the padding string

Upstream `exsltStrAlignFunction` (libexslt `strings.c`) uses the padding
argument itself as the alignment canvas:

```c
str_l = xmlUTF8Strlen(str);
padding_l = xmlUTF8Strlen(padding);
if (str_l == padding_l)   return str;
if (str_l >  padding_l)   return xmlUTF8Strndup(str, padding_l);
if (right)   ret = xmlUTF8Strndup(padding, padding_l - str_l) + str;
else if (center) {
    left = (padding_l - str_l) / 2;
    ret = xmlUTF8Strndup(padding, left) + str;
    ret += padding + xmlUTF8Strsize(padding, left + str_l);
} else {                                  /* left */
    ret = str + padding + xmlUTF8Strsize(padding, str_l);
}
```

The candidate filled with literal spaces and used a different centre bias
(`div_ceil`), so `str:align(string(.), '***', 'center')` returned `' B '`
instead of `'*B*'`. It now slices the padding string by characters and uses
upstream's floor centre.

## Root cause 3 — `math:constant` ignored its precision argument

Upstream `exsltMathConstant` (libexslt `math.c`) keeps each constant as a
decimal **string**, truncates it to `min(strlen(constant), (int)precision)`
characters, and parses that prefix:

```c
#define EXSLT_PI "3.1415926535897932384626433832795028841971693993751"
...
len = xmlStrlen(EXSLT_PI);
if (precision <= len) len = (int)precision;
str = xmlStrsub(EXSLT_PI, 0, len);
ret = xmlXPathCastStringToNumber(str);
```

So `math:constant('PI', count(*)+2)` yields `3.14` at precision 4 and `3` at
precision 2. The candidate returned the full `f64` constant for every
precision, and ignored the name case-sensitivity. The constant set is also
`PI, E, SQRRT2, LN2, LN10, LOG2E, SQRT1_2` — the candidate's `LOG10E` does not
exist upstream and `SQRT1_2` did. Both are now correct.

## Unit tests updated

`exslt::math::tests::test_constant` and `exslt::strings::tests::test_align`
asserted the pre-fix behaviour (full-precision constant; right-biased centre).
They now assert the upstream contract, including the NaN cases (unknown name,
precision < 1, missing precision).
