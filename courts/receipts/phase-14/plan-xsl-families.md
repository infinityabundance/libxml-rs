# plan-xsl — engine root-cause FAMILY map (57 head tests)

Method: read each member's captured `.diff` (candidate output) against its `.exp` (oracle =
libxslt 1.1.45 / libxml2 2.15.3, passes). `-` lines = oracle-expected content missing in candidate;
`+` lines = candidate-actual extra output/diagnostics. Classified by observed divergence ⇒ mapped
to the engine surface in `libxml-rs`. Prime families concrete on PHP-callback routing,
param/global data binding, transformTo* result-doc lifetime, and EXSLT/ns registration.
Obs-Severity: **F**=php-callback-`route` (function-resolution/registration/return), **P**=param
plumbing, **C**=crash/ownership, **M**=output/serialize mismatch, **E**=message-text parity.

A ~31-test cross-cutting spurious warning (see **XLOAD**) pollutes the first `.diff` line of nearly
every `prepare.inc`-style test; XLOAD is a *parser/DOM-load* parity item (ext/dom + ext/xml),
not an XSLT-engine bug, and is the single highest-leverage prerequisite in this suite.

---

## XLOAD — spurious `DOMDocument::load(): Invalid bytes … xslt.xml, line: 20` (loader encoding parity; FLAG extends to ext/dom+xml)
Oracle libxml2 loads `ext/xsl/tests/xslt.xml` (contains byte `0xD3 '3'`, a non-UTF8 sequence) cleanly
and emits **no** warning; libxml-rs's load path UTF-8-validates and warns. On real libxml2 the same
`prepare.inc` produces clean output for these tests. Observed as an exact `+`-only extra.
*Engine surface: `src/xml/parser` encoding/UTF-8 "Invalid bytes" detection vs byte-opaque xmlChar load.
Purely behavioral-divergence (does not corrupt later logic for these tests).*
**Members gated primarily by XLOAD alone (bodies otherwise already match oracle):**
`xsltprocessor_getParameter.phpt`, `xsltprocessor_getParameter-invalidparam.phpt`,
`xsltprocessor_removeParameter.phpt`, `xsltprocessor_removeParameter-invalidparam.phpt`,
`xsltprocessor_setparameter-nostring.phpt`.
**Members that 1st-line XLOAD but keep a real 2nd divergence (listed under their true family):**
bug48221, bug54446, bug54446_with_ini, registerPHPFunctionNS, req30622, setParameter_exceptions_test,
xslt001..xslt007, xslt012, xsltprocessor_registerPHPFunctions-* (10), setparameter-errorquote.
**→ Fix XLOAD first: it retroactively shrinks many listed member counts to "already green core".**

---

## H0 — Rust/UTF-8 edge panics + recursion/depth crash (highest priority, must land first)
- **H0a char-boundary slice panic** — crash family; member `bug26384.phpt` (1). Diff shows engine
  panic `src/xml/xpath/functions.rs:359:32 end byte index 1 not a char boundary` rising through
  `xslt::keys::build_key_table`→`xmlXPathEvalExpression`: an xpath string fn sliced a multi-byte
  UTF-8 (Cyrillic) argument mid-`translate`/`substring` while compiling an `xsl:key` use attr.
  *Engine: byte-vs-char index bug in xpath functions under key-table build — pure libxml-rs.*
- **H0b `bug71571_a/b`** (2). `a` expected `xsltApplySequenceConstructor: potential infinite template
  recursion` + Templates/Variables diagnostics (message-text only); `b` = **`Segmentation fault`**
  with `maxTemplateDepth=2**30; maxTemplateVars=2`. Root: the `maxTemplateDepth`/`maxTemplateVars`
  recursion guard counts/limits and their PHP setters, plus the crash path when the vars limit is set
  very high — ownership-guard in `xsltApplyXSLTTemplate` var pushing. *C/E, engine `src/xslt/transform`.
  Prereq: proper property setter validation land here (H3) so b does not UB.*
- **H3 property setter/value clamping (`maxTemplate*`, typed props)** — `maxTemplateVars_modification_validation_bypass.phpt`,
  `special_operations_with_properties.phpt` (2). Bypass: `$maxTemplateDepth` dump shows `int(30000)`
  vs oracle `int(3000)` — the ini/`setProperty` override window isn't clamped (ValueError/`>=0`) and
  typed-property read/unset semantics differ. *E/V, PHP xsl object-property emulation + `AUTO` value
  clamp; independent of engine XSLT but gates H0b severity.*
- **bug49634** exception-crash class: currently fails at `Unregistered function ext:function` (see F1);
  once routing lands it becomes a transformToDoc exception/ownership re-check. *F→C re-flaggable.*

## F1 — `php:function` / `registerPHPFunctions` / `registerPHPFunctionNS` routing → PSYN route into engine
Engine never resolves `php:function(...)`/`php:functionString`/`ext:function` and the PHP-callable
table set via register* is never bridged into the compiled stylesheet's XPath context, so every
selector is reported **`Unregistered function: php:function`** and produces an empty/identity doc,
often leaving raw leftover mangled `P;…:value-of` text via a broken prefix. Output-mismatch + missing
runtime handler-*diagnostics* + empty result all collapse to this one bridge.
*Engine surface: reading the ns-ext + function table the PHP layer registers into the XSLT ctx before
`xsltApplyStylesheetUser` (the php_xsl internal registration currently registers into a ctx the engine
sees separately; callsite appears as if never registered) AND the feedback to apply-template compiles.
Member (primary): `XSLTProcessor_callables.phpt`, `php_function_edge_cases.phpt`,
`registerPHPFunctionNS.phpt`, `auto_registration_namespaces_new_dom.phpt`,
`XSLTProcessor_namespace_mapper_lifetime.phpt`, `bug33853.phpt`, `throw_in_autoload.phpt`,
`bug49634.phpt`, `bug69168.phpt`, `bug70078.phpt`, `cloneDocument.phpt`, `xslt011.phpt`,
`xslt_non_dom_node.phpt`, and ALL ten `xsltprocessor_registerPHPFunctions-{allfuncs,array,
array-multiple,array-notallowed,funcnostring,funcundef,null,string,string-multiple,string-notallowed}.phpt`
(bodies: value-of in `phpfunc.xsl` unresolved). (≈22)
Cross-listed (secondary distractor): `setParameter_exceptions`/`xslt001-007/012` show php:function too.
**Prereq: XLOAD before re-judging member core, and F2 if the php fn is an EXSLT ns override.**

## F2 — EXSLT ext-namespace BUILTIN-function coverage & override registration
`xsltprocessor_exsl_registerPhpFunctionNs.phpt` (1): registerPHPFunctionNS(“http://exslt.org/…”,
`year`, dummy) must override the builtin; candidate ignores the override → prints builtin `2007`
twice. `xslt010_gt10129.phpt` (1): EXSLT `date:*` XPath functions (`date:day-in-week` etc.) unresolved
→ “Test 10: EXSLT Support” stalls. Root: EXSLT “dates-and-times” function table partially
present/registered (some date fns run, others never hook ext-lookup) + ns-override-before-builtin
semantics. *Engine: `src/xslt` EXSLT function registration + lookup ordering (builtins vs override).*

## F-doc — result-document lifecycle (`transformToDoc/URI/XML` → doc where/psyn)
`transformToDoc_sxe_type_error.phpt` (result DOM ends empty then the SXE wrapper `#4(0)`), 
`xsltprocessor_transformToDoc.phpt` (firstChild/tagName NULL from a false result), 
`xsltprocessor_transformToURI.phpt` (`int(-1)` failed write — expected 56), 
`xsltprocessor_transformToXML.phpt` (bool(false) + empty — overlay due F1 unresolved value-of). Root:
the doc-handle the transform writes to isn’t connected/lifetime-kept & child is detached/absent until
the callback-F1 route returns a node. Also the SXE/URI target needing result serialization node.
*Engine: transformTo* result-document ownership handoff in `src/xslt/transform` + saving to the URI
target. Requires F1 first (docs are populated by ext/function results).*  (4)

## M1 — namespace mapper + `xsl:element@xmlns`/`local-name` + `node()` pattern resolution
`gh21357_2.phpt` (1): identity+`<xsl:element name="{local-name()}" xmlns="old→new">` fails &
reports `Unregistered function: node` from an `apply-templates` on the pattern — the engine resolves
an unprefixed *pattern/axis-function* name in the stylesheet default-ns space instead of recognition
of the XPath node() set. Shares half the surface with F1 (unqualified function name lookup under a
new default ns) but the primary fixed test is pure namespace reconciliation & `xsl:element` default-ns
rewrite = N-family. *Needs src/xml ns-commit + src/xpath function/ns resolution parity.*

## P1 — setParameter/getParameter/removeParameter validation + value re-quoting + NS-NULL entry
Members: `bug64137.phpt` (quoted value `$foo` empty for all incl. `""`), `bug48221.phpt` (quote-heavy
`setParameter('','','"\'')` → `Could not apply parameter ""`; +XLOAD), `req30622.phpt` (namespace-“NULL”
and test-ns set-vals read back EMPTY — param persist under NULL/ns keys broken), 
`setParameter_exceptions_test.phpt` (numeric-`1` value not applied / Error path), 
`xsltprocessor_setparameter-errorquote.phpt` (errorquote + Could-not-apply), plus the pure-XLOAD-fails
get/remove/set members above (move off once XLOAD fixed). Root: PHP-store-to-engine param binding +
string→XPath-literal **re-quote escaping** (the raw `'`/`"` value round-trips through an expression
string that must be escaped; empty for quote-only values = escaping bug). *E/P, `src/xslt` param table
& `xsltVarValue` literalize; ALSO xslt003/xslt012 (assoc array value + the foo: variant) show a param
value that should inject not applied → same root, cross-listed with apply.*  (~7 primary)

## E1 — importStylesheet node-type gate & “not a stylesheet” compile path
`gh21496.phpt` (1): importStylesheet(DOMComment with text “my value”) and a SimpleXMLElement must warn
`compilation error … document is not a stylesheet` + return `bool(false)`; candidate returns
`bool(true)` with no warning. Root: driver not rejecting a bare-Comment/root-non-stylesheet arg through
`xsltParseStylesheetProcess` with those diagnostics. *E, message-text + validation gate on top-level.*

## I1 — `xsl:include` / `xsl:import` relative-URI (base `file://`) + HTML-output writer
`bug53965.phpt` (1): include.xsl included from a `file://…` loaded collection.xsl not located → CD
template never brought in, output loses the wrapped rows. Root: include/import href resolved against
the base of a `file://` doc & the include sub-doc compile. *Engine include loader in `src/xslt` incl.
correct relative resolution + applied subtree. Secondary M/serialize (html `method`) interplay.*

## XD — extension-element write `xsl:document`/`sax:output→file` + writer refusal errors
`bug54446.phpt`, `bug54446_with_ini.phpt` (2): expected `sax:output href=…file` write-refused /
`xsltDocumentElem write rights denied` warnings (**no file allowed to be created**); candidate writes
nothing, leaves the literal `<sax:output …>` markup text, later `file_get_contents()` fails, and no
file exists. Root: `xsl:document`/`sax:document` extension-element handler + per-write rights-check
(`xsltDocumentElem`) + text-method file output not implemented; and the write-rights diagnostic.
Order-independent of XLOAD? These two also 1st-line XLOAD (they don't setParameter forward? they use
prepare? bug54446 includes prepare? checked: uses its own xml) — both first-line XLOAD actually came
from a separate load in test - let me recheck: bug54446.diff's first +-line was XLOAD too; body has no
prepare? The body loads xsl+xml explicitly + a custom .xsl with sax:output; XLOAD line points to xslt.xml
… bug54446 must include('prepare.inc'). It doesn't (I saw only files w/ sax). Mark unverified so the
XLOAD heading is honest: verify body). Root engine D/element **output file creation + write-rights**,
independent of PSYN/params — engine `src/xslt` `sax:document`/`output`.

## Apply / selection engine-edge (XLOAD-blocked; pattern param divergence — **partially unverified**)
`xslt001`, `xslt002`, `xslt003`, `xslt004`, `xslt005`, `xslt006`, `xslt007`, `xslt012.phpt` (8): all
share the XLOAD first-line warning but also fail to reproduce the template-application rows/params
(expected `a1 b1 c1…` / assoc-assoc param `hello world`/`barbar` vs candidate keeping the default
`bar`). Whether row-loss is loader-truncation after the invalid-byte warning (would clear with XLOAD)
vs a genuine apply/`xsl:for-each`/param injection gap only XLOAD+param unit test isolates. **Marked
partially-unverified: primary hypothesis = XLOAD truncates parse → then P1/subapply.** Keep them as a
dedicated regression surface for the apply for-each + html result assembly to check post-XLOAD.

---

## Ordering & prerequisite flags per root-cause
1. **XLOAD parser-load parity** (cross ext/dom+xml) — retroactively greens or isolates ~5 +1st-line of
   ~26 members; do first (cheap, non-XSLT engine change, no regression risk to real-loader semantics
   since it must stop UTF-8-warning only where libxml2 is silent).
2. **H0a UTF-8 slice panic + H2-b recursion guard/limits** (crash/UB; ub-blocking) — pure engine.
3. **H3 property setter validation (maxTemplateDepth/Vars clamps, typed props)** → needed for correct
   H0b behavior/diagnostics and the two property tests.
4. **F1 PHP-callback routing bridge** — the parent of the room; greens ~22–26 members, then re-audit
   bug49634 (exception/ownership), cloneDocument, transformTo* result chaining.
5. **F2 EXSLT builtin/override registration** (date:*, ns-override) → unblocks xslt010 + register-ns
   override (depends on F1 route table for the override to hit PHP).
6. **P1 param/global binding + re-quote** (after F1 lands so selectors run and params inject into
   value-of on source) — also clears xslt003/012 & recurse-params.
7. **F-doc transformToDoc/URI/XML result-doc ownership** + serializer save parity; **M1** ns-mapper
   `<xsl:element xmlns>` after `src/xml` ns fixes; **E1** importStylesheet gate; **I1** include/import
   relative; **XD** xsl:document write — independent engine edges, parallelize/schedule by code-path
   isolation (each leaves a monolith intact).

**Flags:** P1/get-parameter family FIRST-LINE depends on ext/dom+xml load/encoding parity (XLOAD); the
transformTo*/bug49634 result members depend on `ext/dom` DOM node lifetime/namespace fixes for
`Dom\XMLDocument` sources; xslt00x/012 + M1 depend on xpath pattern/`node()` resolution depth &
namespace commit; F2/serialize depend on serializer/save parity for text/html-method & `output`.
**unverified** members whose *secondary* divergence I could not byte-verify (body/diff mismatch may be
loader-truncation): xslt001–xslt007, xslt012, bug54446(_with_ini) row-content detail; bug53855 empty
h1/h2 parts. (Verify = re-run after XLOAD.)
