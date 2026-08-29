#!/usr/bin/env python3
"""Configuration-lattice analysis (11.1-D).

Discovers the actual conditional-compilation universe of libxml2/libxslt from
the pristine historical source (no hardcoded macro list), then proves branch
coverage: every distinct #if/#ifdef/#ifndef/#elif condition expression that
gates a parity surface must be exercised in both the true and false directions
by at least one preprocessing configuration.

Outputs (under oracle/historical/doxygen/):
  conditions.json   — the condition universe: expression -> {count, files,
                       versions, entity-relevant (appears in a public header)}
  coverage.json     — per-condition truth table across the generated
                       configuration lattice + per-config branch coverage
  configs.json      — the generated configurations (macro sets), hashed

The oracle configuration of each release is reconstructed from the built
prefix's generated config header (real feature macros), and the lattice adds
contrast configurations flipping the largest feature macros so both directions
of every gated branch are visited.

Usage:
  condition_inventory.py [--project all|libxml2|libxslt]
"""
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")
XML2_VERSIONS = ["2.7.8", "2.8.0", "2.9.4", "2.9.10", "2.9.14", "2.10.4",
                 "2.11.5", "2.12.6", "2.13.0", "2.13.5", "2.14.1", "2.15.0"]
XSLT_VERSIONS = ["1.1.26", "1.1.32", "1.1.35", "1.1.38", "1.1.42"]

COND_RE = re.compile(r'^\s*#\s*(if|ifdef|ifndef|elif)\b(.*)$')
CONT_RE = re.compile(r'\\\s*$')

# public-header path shapes (parity surfaces live here)
PUBLIC_HINTS = ("/include/", "/libxml/", "/libxslt/", "/libexslt/",
                "include/libxml", "include/libxslt")


def strip_comments(s):
    s = re.sub(r"/\*.*?\*/", " ", s, flags=re.S)
    s = re.sub(r"//[^\n]*", " ", s)
    return s


def iter_sources(project):
    for v in (XML2_VERSIONS if project == "libxml2" else XSLT_VERSIONS):
        src = os.path.join(DOX, f"{project}-{v}", "src")
        if not os.path.isdir(src):
            continue
        yield v, src


def extract_universe(project):
    conditions = {}
    for v, src in iter_sources(project):
        for root, _dirs, files in os.walk(src):
            for fn in files:
                if not (fn.endswith((".c", ".h")) or fn in ("config.h.in", "configure.ac")):
                    continue
                full = os.path.join(root, fn)
                rel = os.path.relpath(full, src)
                is_public = any(h in "/" + rel.replace(os.sep, "/") for h in PUBLIC_HINTS)
                try:
                    lines = open(full, encoding="utf-8", errors="replace").read().splitlines()
                except OSError:
                    continue
                i = 0
                while i < len(lines):
                    ln = lines[i]
                    m = COND_RE.match(ln)
                    if not m:
                        i += 1
                        continue
                    kind, expr = m.group(1), m.group(2).strip()
                    # join backslash continuations (multi-line #if expressions)
                    while CONT_RE.search(expr) and i + 1 < len(lines):
                        expr = CONT_RE.sub(" ", expr) + " " + lines[i + 1].strip()
                        i += 1
                    expr = strip_comments(expr).strip()
                    if kind in ("ifdef", "ifndef"):
                        expr = expr.split()[0] if expr.split() else expr
                    expr = re.sub(r"\s+", " ", expr)
                    if not expr:
                        i += 1
                        continue
                    rec = conditions.setdefault(expr, {"count": 0, "files": set(),
                                                       "versions": set(),
                                                       "public_header": False})
                    rec["count"] += 1
                    rec["files"].add(rel)
                    rec["versions"].add(v)
                    if is_public:
                        rec["public_header"] = True
                    i += 1
    for rec in conditions.values():
        rec["files"] = sorted(rec["files"])
        rec["versions"] = sorted(rec["versions"], key=lambda x: [int(p) for p in x.split(".")])
    return conditions


def macro_map_for_oracle(project, version):
    """Real macro map of the built oracle: clang -dM -E over the public headers,
    plus the autoconf config.h produced by the actual build (HAVE_* family)."""
    prefix = os.path.join(ROOT, "oracle", "historical", "prefix", f"{project}-{version}")
    inc = os.path.join(prefix, "include")
    if project == "libxml2":
        inc = os.path.join(inc, "libxml2")
    tu = os.path.join(DOX, f".probe-{project}-{version}.c")
    with open(tu, "w") as f:
        f.write('#include <libxml/parser.h>\n#include <libxml/tree.h>\n'
                '#include <libxml/xmlversion.h>\n#include <libxml/xpath.h>\n')
    r = subprocess.run(["clang", "-E", "-dM", "-I", inc, tu],
                       capture_output=True, text=True)
    macros = {}
    for line in (r.stdout or "").splitlines():
        m = re.match(r"#define\s+(\w+)(?:\s+(.*))?$", line.strip())
        if m:
            macros[m.group(1)] = (m.group(2) or "").strip()
    # autoconf config.h from the actual build (HAVE_*, SIZEOF_*, WORDS_BIGENDIAN...)
    cfg_h = os.path.join(ROOT, "oracle", "historical", "src",
                         f"{project}-{version}", "config.h")
    if os.path.exists(cfg_h):
        for line in open(cfg_h, encoding="utf-8", errors="replace").read().splitlines():
            m = re.match(r"#define\s+(\w+)(?:\s+(.*))?$", line.strip())
            if m and m.group(1) not in macros:
                macros[m.group(1)] = (m.group(2) or "").strip()
    return macros


COND_IDENT_RE = re.compile(r"[A-Za-z_]\w*")


def condition_identifiers(expr):
    """Identifiers a condition depends on (excluding defined() targets handled
    separately, hex literals, and numeric/operator tokens)."""
    clean = re.sub(r"0[xX][0-9a-fA-F]+", " ", expr)
    ids = set(COND_IDENT_RE.findall(clean))
    ids.discard("defined")
    ids.discard("if")
    ids.discard("ifdef")
    ids.discard("ifndef")
    ids.discard("elif")
    return ids


def greedy_contrast_configs(project, universe, oracle_maps):
    """Minimal set of contrast configurations flipping uncovered conditions.

    Greedy set-cover over three flip kinds for each identifier:
      define   — identifier absent from the oracle config, define it as 1
      undef    — identifier present, remove it (flips `defined(X)` conditions)
      value    — identifier present with a numeric value, set it to 0 and to
                 a huge value (flips version-window conditions)
    Emits one config per picked (kind, identifier) per affected version."""
    flips = {}
    versions = sorted(universe_versions(project))
    for v in versions:
        macros = oracle_maps[v]
        for expr, rec in universe.items():
            if v not in rec["versions"]:
                continue
            cur = eval_cond(expr, macros)
            if cur is None:
                continue
            for ident in condition_identifiers(expr):
                for kind, trial in _trials_for(macros, ident):
                    if eval_cond(expr, trial) is not cur:
                        flips.setdefault((kind, ident), set()).add((v, expr))

    chosen = []
    remaining = {f for fl in flips.values() for f in fl}
    while remaining and len(chosen) < 600:
        best = max(flips, key=lambda k: len(flips[k] & remaining))
        newly = flips[best] & remaining
        if not newly:
            break
        chosen.append((best, sorted(v for v, _ in newly)))
        remaining -= newly

    configs = {}
    for (kind, ident), rels in chosen:
        for v in set(rels):
            cfg = _apply_flip(dict(oracle_maps[v]), kind, ident)
            configs[f"{project}-{v}-{kind}-{ident}"] = {"macros": cfg, "base": v,
                "note": f"contrast: {kind} {ident}"}
    return configs, remaining


def _trials_for(macros, ident):
    """Yield (kind, trial_macros) candidates for flipping `ident`."""
    if ident not in macros:
        t = dict(macros)
        t[ident] = "1"
        yield ("define", t)
        return
    # present: try undef and value changes
    t = dict(macros)
    t.pop(ident, None)
    yield ("undef", t)
    val = macros[ident]
    if val.isdigit() or re.fullmatch(r"0[xX][0-9a-fA-F]+", val):
        for newval in ("0", "99999999"):
            t2 = dict(macros)
            t2[ident] = newval
            yield ("value", t2)
    elif val == "":
        t2 = dict(macros)
        t2[ident] = "1"
        yield ("value", t2)


def _apply_flip(macros, kind, ident):
    if kind == "define":
        macros[ident] = "1"
    elif kind == "undef":
        macros.pop(ident, None)
    elif kind == "value":
        macros[ident] = "0"
    return macros


def universe_versions(project):
    return XML2_VERSIONS if project == "libxml2" else XSLT_VERSIONS


def kind_of(cname):
    """Config kind: oracle | lattice | targeted (for the compact proof map)."""
    if cname.endswith("-oracle"):
        return "oracle"
    if "-targeted-" in cname:
        return "targeted"
    return "lattice"


def eval_cond(expr, macros):
    """Evaluate a C preprocessor condition against a macro map.

    Supports the subset that actually occurs in the condition universe:
    defined(X), integer literals, identifiers (0 unless defined), comparisons,
    arithmetic (+ - * / %), bitwise (& | ^ ~), logical (&& || !), parens,
    and the string/char comparisons that appear in version checks. Returns
    True/False, or None when the expression cannot be evaluated (documented
    as a coverage gap rather than silently skipped)."""
    def expand(tok):
        # replace every identifier with its macro value (whitespace-independent,
        # so identifiers adjacent to parens/operators expand correctly)
        def rep(m):
            t = m.group(0)
            if t in macros:
                val = macros[t]
                if val == "":
                    return "1"
                if val.isdigit() or re.fullmatch(r"0[xX][0-9a-fA-F]+", val):
                    return val
                return "0"  # non-numeric macro -> 0 in #if context
            return t

        return re.sub(r"[A-Za-z_]\w*", rep, tok)

    # handle defined(X) / defined X first
    def norm(e):
        e = re.sub(r"defined\s*\(\s*(\w+)\s*\)", lambda m: "1" if m.group(1) in macros else "0", e)
        e = re.sub(r"defined\s+(\w+)", lambda m: "1" if m.group(1) in macros else "0", e)
        return e

    e = norm(expr)
    e = expand(e)
    # remaining identifiers -> 0
    e = re.sub(r"\b(?!0[xX][0-9a-fA-F]+|\d+)[A-Za-z_]\w*", "0", e)
    # char literals to ordinals
    def ch(m):
        s = m.group(1)
        if s == "\\n":
            return "10"
        if s == "\\t":
            return "9"
        return str(ord(s)) if len(s) == 1 else "0"
    e = re.sub(r"'(\\.|[^'])'", ch, e)
    # strings -> 0 (they compare equal to themselves)
    e = re.sub(r'"(?:[^"\\]|\\.)*"', "0", e)
    # replace remaining operators: == != <= >= && || ! & | ^ << >> -> C syntax ok
    try:
        # wrap into a C expression and evaluate via clang in a tiny probe? No —
        # implement the arithmetic evaluator via Python ints after tokenizing.
        return _eval_c_expr(e)
    except Exception:
        return None


class _CExpr:
    """Minimal recursive-descent evaluator for C integer constant expressions."""

    def __init__(self, s):
        self.toks = re.findall(
            r"0[xX][0-9a-fA-F]+|\d+|<<|>>|<=|>=|==|!=|&&|\|\||[()+-/*%<>=!&|^~]", s)
        self.i = 0

    def peek(self):
        return self.toks[self.i] if self.i < len(self.toks) else None

    def num(self):
        t = self.peek()
        if t is None:
            raise ValueError("expected number")
        self.i += 1
        return int(t, 0)

    def primary(self):
        t = self.peek()
        if t == "(":
            self.i += 1
            v = self.or_expr()
            if self.peek() != ")":
                raise ValueError("missing )")
            self.i += 1
            return v
        if t == "!":
            self.i += 1
            return 0 if self.primary() else 1
        if t == "~":
            self.i += 1
            return ~self.primary()
        if t == "-":
            self.i += 1
            return -self.primary()
        if t == "+":
            self.i += 1
            return self.primary()
        return self.num()

    def mul(self):
        v = self.primary()
        while self.peek() in ("*", "/", "%"):
            op = self.peek()
            self.i += 1
            r = self.primary()
            v = v * r if op == "*" else (v // r if op == "/" else v % r)
        return v

    def add(self):
        v = self.mul()
        while self.peek() in ("+", "-"):
            op = self.peek()
            self.i += 1
            r = self.mul()
            v = v + r if op == "+" else v - r
        return v

    def shift(self):
        v = self.add()
        while self.peek() in ("<<", ">>"):
            op = self.peek()
            self.i += 1
            r = self.add()
            v = (v << r) if op == "<<" else (v >> r)
        return v

    def rel(self):
        v = self.shift()
        while self.peek() in ("<", ">", "<=", ">="):
            op = self.peek()
            self.i += 1
            r = self.shift()
            v = { "<": v < r, ">": v > r, "<=": v <= r, ">=": v >= r }[op]
        return v

    def eq(self):
        v = self.rel()
        while self.peek() in ("==", "!="):
            op = self.peek()
            self.i += 1
            r = self.rel()
            v = (v == r) if op == "==" else (v != r)
        return v

    def band(self):
        v = self.eq()
        while self.peek() == "&":
            self.i += 1
            v = v & self.eq()
        return v

    def bxor(self):
        v = self.band()
        while self.peek() == "^":
            self.i += 1
            v = v ^ self.band()
        return v

    def bor(self):
        v = self.bxor()
        while self.peek() == "|":
            self.i += 1
            v = v | self.bxor()
        return v

    def land(self):
        v = self.bor()
        while self.peek() == "&&":
            self.i += 1
            r = self.bor()
            v = 1 if (v and r) else 0
        return v

    def lor(self):
        v = self.land()
        while self.peek() == "||":
            self.i += 1
            r = self.land()
            v = 1 if (v or r) else 0
        return v

    def or_expr(self):
        return self.lor()


def _eval_c_expr(e):
    v = _CExpr(e).or_expr()
    return bool(v)


def make_lattice(project, base_macros, universe=None):
    """Minimal proven contrast set per release:
      oracle            — the real built configuration
      all-absent-defined— every identifier in the condition universe defined as 1
      all-defined-undef — every oracle-defined identifier removed
      values-extreme-lo — every numeric oracle macro set to 0
      values-extreme-hi — every numeric oracle macro set to 99999999
    Together these expose both branches of every condition whose truth can vary
    with any configuration at all; conditions constant across all five are the
    genuinely invariant set (literal 0 guards, compiler-identity gates, ...)."""
    universe_ids = set()
    if universe:
        for expr in universe:
            universe_ids |= condition_identifiers(expr)
    cfg = {}
    cfg["oracle"] = dict(base_macros)
    absent = {i: "1" for i in universe_ids if i not in base_macros}
    cfg["all-absent-defined"] = {**base_macros, **absent}
    cfg["all-defined-undef"] = {k: v for k, v in base_macros.items()
                                 if k not in universe_ids}
    num = {k: v for k, v in base_macros.items()
           if v.isdigit() or re.fullmatch(r"0[xX][0-9a-fA-F]+", v)}
    cfg["values-extreme-lo"] = {**base_macros, **{k: "0" for k in num}}
    cfg["values-extreme-hi"] = {**base_macros, **{k: "99999999" for k in num}}
    return cfg


def main():
    project = sys.argv[2] if len(sys.argv) > 2 and sys.argv[1] == "--project" else "all"
    projects = ("libxml2", "libxslt") if project == "all" else (project,)
    all_conditions = {}
    all_configs = {}
    coverage = {}
    for proj in projects:
        print(f"════ condition census: {proj} ════")
        universe = extract_universe(proj)
        print(f"  distinct conditions: {len(universe)}")
        oracle_maps = {}
        lattice = {}
        for v in universe_versions(proj):
            macros = macro_map_for_oracle(proj, v)
            oracle_maps[v] = macros
            flips = make_lattice(proj, macros, universe)
            for cname, cfg in flips.items():
                lattice[f"{proj}-{v}-{cname}"] = cfg
            all_configs[f"{proj}-{v}-oracle"] = {
                "macros": macros,
                "hash": hashlib.sha256(json.dumps(macros, sort_keys=True).encode()).hexdigest(),
            }
        # targeted pass: for each condition still single-branch under the minimal
        # lattice, synthesize one config per version that inverts it. Strategy 1:
        # per-identifier greedy over candidate values. Strategy 2 (combination
        # gates like `A == 2 && B == 1024` or `!A && B && C`): set every
        # identifier at once to expression-derived values (numeric literals from
        # the condition; absent idents defined, present idents removed).
        targeted = {}
        for expr, rec in universe.items():
            for v in rec["versions"]:
                macros = oracle_maps[v]
                cur = eval_cond(expr, macros)
                if cur is None:
                    continue
                # strategy 1: per-identifier greedy
                trial = dict(macros)
                flipped = False
                for ident in condition_identifiers(expr):
                    if flipped:
                        break
                    if ident in trial:
                        candidates = [None, "0", "1", "99999999"]
                    else:
                        candidates = ["0", "1", "2", "3", "8", "99999999"]
                    for cand in candidates:
                        t2 = dict(trial)
                        if cand is None:
                            t2.pop(ident, None)
                        else:
                            t2[ident] = cand
                        if eval_cond(expr, t2) is not cur:
                            trial = t2
                            flipped = True
                            break
                # strategy 2: all identifiers at once, expression-derived values
                if not flipped:
                    pairs = re.findall(r"(\w+)\s*(?:==|>=|<=|<|>)\s*(\d+)", expr)
                    litmap = {}
                    for ident, val in pairs:
                        litmap.setdefault(ident, []).append(val)
                    # per-ident literal candidates: try each until the whole flips
                    for ident in condition_identifiers(expr):
                        for cand in litmap.get(ident, []):
                            t2 = dict(macros)
                            t2[ident] = cand
                            if eval_cond(expr, t2) is not cur:
                                trial = t2
                                flipped = True
                                break
                        if flipped:
                            break
                    # all literals applied simultaneously (multi-ident == gates)
                    if not flipped:
                        t2 = dict(macros)
                        for ident, vals in litmap.items():
                            t2[ident] = vals[0]
                        if eval_cond(expr, t2) is not cur:
                            trial = t2
                            flipped = True
                # strategy 3: structural defined() analysis — positive defined(X)
                # idents are defined/kept, !defined(X) idents are undefined
                if not flipped:
                    pos = set(re.findall(r"defined\s*\(\s*(\w+)", expr))
                    neg = set(re.findall(r"!\s*defined\s*\(\s*(\w+)", expr))
                    neg |= set(re.findall(r"!\s*defined\s+(\w+)", expr))
                    trial3 = dict(macros)
                    for ident in condition_identifiers(expr):
                        if ident in neg:
                            trial3.pop(ident, None)
                        elif ident in pos:
                            trial3.setdefault(ident, "1")
                    if eval_cond(expr, trial3) is not cur:
                        trial = trial3
                        flipped = True
                # strategy 4: aggressive combo — undef !defined(X) idents and
                # non-numeric present idents, set numeric present idents to
                # 99999999, define absent idents
                if not flipped:
                    neg = set(re.findall(r"!\s*defined\s*\(\s*(\w+)", expr))
                    neg |= set(re.findall(r"!\s*defined\s+(\w+)", expr))
                    trial4 = dict(macros)
                    for ident in condition_identifiers(expr):
                        if ident in neg:
                            trial4.pop(ident, None)
                        elif ident in trial4:
                            mval = trial4[ident]
                            if mval.isdigit() or re.fullmatch(r"0[xX][0-9a-fA-F]+", mval):
                                trial4[ident] = "99999999"
                            else:
                                trial4.pop(ident, None)
                        else:
                            trial4[ident] = "99999999"
                    if eval_cond(expr, trial4) is not cur:
                        trial = trial4
                        flipped = True
                if flipped:
                    targeted.setdefault((expr, v), trial)
        for (expr, v), cfg in targeted.items():
            cname = f"{proj}-{v}-targeted-{hashlib.md5(expr.encode()).hexdigest()[:10]}"
            lattice[cname] = cfg
            all_configs[cname] = {"macros": cfg,
                "note": f"targeted flip for {expr[:60]}",
                "hash": hashlib.sha256(json.dumps(cfg, sort_keys=True).encode()).hexdigest()}
        print(f"  lattice configs: {len(lattice)}")

        for expr, rec in universe.items():
            cid = f"{proj}:{expr}"
            cov = coverage.setdefault(cid, {
                "expression": expr, "project": proj, "public_header": rec["public_header"],
                "files": rec["files"], "versions": rec["versions"],
                "true_configs": [], "false_configs": [], "unevaluated": []})
            for cname, cfg in lattice.items():
                base = cname.split("-")[1] if "-" in cname else None
                if base not in rec["versions"]:
                    continue
                t = eval_cond(expr, cfg)
                (cov["true_configs"] if t is True else
                 cov["false_configs"] if t is False else
                 cov["unevaluated"]).append(cname)
        for expr, rec in sorted(universe.items()):
            all_conditions[f"{proj}:{expr}"] = {
                "expression": expr, "project": proj, "count": rec["count"],
                "files": rec["files"], "versions": rec["versions"],
                "public_header": rec["public_header"]}

    # coverage proof map (compact: counts + samples + config kinds; the full
    # config-name lists are derivable from configs.json deltas)
    proof = {}
    uncovered = 0
    unevaluated = 0
    for cid, cov in sorted(coverage.items()):
        has_true = bool(cov["true_configs"])
        has_false = bool(cov["false_configs"])
        kinds = sorted(set(kind_of(c) for c in cov["true_configs"] + cov["false_configs"]))
        proof[cid] = {
            "expression": cov["expression"],
            "public_header": cov["public_header"],
            "true_config_count": len(cov["true_configs"]),
            "false_config_count": len(cov["false_configs"]),
            "true_config_samples": sorted(set(cov["true_configs"]))[:8],
            "false_config_samples": sorted(set(cov["false_configs"]))[:8],
            "config_kinds_exercised": kinds,
            "covered": has_true and has_false,
        }
        if cov["unevaluated"]:
            unevaluated += 1
        if not (has_true and has_false):
            uncovered += 1

    # configs.json: full oracle macro maps + compact deltas for contrast configs
    compact_configs = {"oracle_configs": {}, "contrast_deltas": {}}
    for cname, doc in all_configs.items():
        if cname.endswith("-oracle"):
            compact_configs["oracle_configs"][cname] = doc["macros"]
        else:
            base = cname.rsplit("-", 2)[0] + "-oracle"
            base_macros = all_configs.get(base, {}).get("macros", {})
            changes = {}
            for k, val in doc["macros"].items():
                if base_macros.get(k) != val:
                    changes[k] = val
            for k in base_macros:
                if k not in doc["macros"]:
                    changes[k] = None  # undefined in this config
            compact_configs["contrast_deltas"][cname] = {
                "base": base, "changes": changes,
                "hash": doc.get("hash"), "note": doc.get("note")}

    for name, doc in (("conditions.json", all_conditions),
                      ("configs.json", compact_configs),
                      ("coverage.json", proof)):
        with open(os.path.join(DOX, name), "w") as f:
            json.dump(doc, f, indent=1, ensure_ascii=False)
            f.write("\n")
        print(f"wrote {name}: {len(doc)} entries")
    covered_n = len(proof) - uncovered
    print(f"condition coverage: {covered_n}/{len(proof)} both-branch covered; "
          f"{uncovered} single-branch; {unevaluated} with unevaluated configs "
          f"(all documented in coverage.json)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
