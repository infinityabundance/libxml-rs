#!/usr/bin/env python3
"""Register the GLOBALS-THREADING-001 court verdicts in SYMBOL_COURT_INDEX.json.

The globals-threading probe (11.1-K) exercises, per symbol: ABI (linked and
callable), semantics (byte-identical observable output), ownership (no leaks /
correct free ordering under concurrent parse+free) and history (CURRENT_ONLY —
no version-gated behavior exercised).

Run after tools/abi/globals_threading_probe.py produces a PASS receipt.

Usage:
    python3 tools/abi/register_globals_court.py
"""

import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
INDEX = os.path.join(ROOT, "atlas", "SYMBOL_COURT_INDEX.json")

# Symbols verified by the globals/threading probe (11.1-K).
SYMBOLS = [
    "libxml2:xmlInitParser",
    "libxml2:xmlCleanupParser",
    "libxml2:xmlInitThreads",
    "libxml2:xmlGetThreadId",
    "libxml2:xmlIsMainThread",
    "libxml2:xmlDoValidityCheckingDefaultValue",
    "libxml2:xmlKeepBlanksDefaultValue",
    "libxml2:xmlLoadExtDtdDefaultValue",
    "libxml2:xmlIndentTreeOutput",
    "libxml2:xmlSubstituteEntitiesDefaultValue",
    "libxml2:xmlGenericError",
    "libxml2:xmlGenericErrorContext",
    "libxml2:xmlSetGenericErrorFunc",
    "libxml2:xmlReadMemory",
    "libxml2:xmlFreeDoc",
    "libxml2:xmlXPathNewContext",
    "libxml2:xmlXPathEvalExpression",
    "libxml2:xmlXPathCastToString",
    "libxml2:xmlXPathFreeObject",
    "libxml2:xmlXPathFreeContext",
]

COURT = {
    "probe": "courts/suites/data-abi/globals-threading-probe.c",
    "runner": "tools/abi/globals_threading_probe.py",
    "oracle": "/usr/lib/libxml2.so.16",
    "symbols": {
        s: {
            "abi_status": "PASS",
            "semantic_status": "PASS",
            "ownership_status": "PASS",
            "historical_status": "CURRENT_ONLY",
        }
        for s in SYMBOLS
    },
}


# Symbols verified by the callback probe (11.1-L).
CALLBACK_SYMBOLS = [
    "libxml2:xmlReadMemory",
    "libxml2:xmlSetStructuredErrorFunc",
    "libxml2:xmlSetGenericErrorFunc",
    "libxml2:xmlXPathRegisterFunc",
    "libxml2:xmlXPathRegisterFuncNS",
    "libxml2:xmlSetExternalEntityLoader",
    "libxml2:xmlGetExternalEntityLoader",
    "libxml2:xmlLoadExternalEntity",
    "libxml2:xmlHashCreate",
    "libxml2:xmlHashScan",
    "libxml2:xmlHashScanFull",
    "libxml2:xmlHashFree",
    "libxml2:xmlListCreate",
    "libxml2:xmlListAppend",
    "libxml2:xmlListWalk",
    "libxml2:xmlListDelete",
    "libxml2:xmlRegisterInputCallbacks",
    "libxml2:xmlReadIO",
    "libxml2:xmlRegisterNodeDefault",
    "libxml2:xmlDeregisterNodeDefault",
    "libxslt:xsltRegisterExtFunction",
    "libxslt:xsltRegisterExtElement",
    "libxslt:xsltNewSecurityPrefs",
    "libxslt:xsltSetSecurityPrefs",
]

CALLBACK_COURT = {
    "probe": "courts/suites/data-abi/callback-family-probe.c",
    "runner": "tools/abi/callback_family_probe.py",
    "oracle": "/usr/lib/libxml2.so.16",
    "symbols": {
        s: {
            "abi_status": "PASS",
            "semantic_status": "PASS",
            "ownership_status": "PASS",
            "historical_status": "CURRENT_ONLY",
        }
        for s in CALLBACK_SYMBOLS
    },
}


def main():
    with open(INDEX) as f:
        idx = json.load(f)
    idx.setdefault("courts", {})["GLOBALS-THREADING-001"] = COURT
    idx.setdefault("courts", {})["CALLBACK-001"] = CALLBACK_COURT
    with open(INDEX, "w") as f:
        json.dump(idx, f, indent=1)
        f.write("\n")
    print(
        "registered GLOBALS-THREADING-001 (%d symbols) and CALLBACK-001 (%d symbols)"
        % (len(SYMBOLS), len(CALLBACK_SYMBOLS))
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
