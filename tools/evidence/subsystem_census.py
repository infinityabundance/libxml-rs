#!/usr/bin/env python3
"""11.1-O — Complete Subsystem Census.

Enumerates every libxml2 / libxslt / EXSLT subsystem called out by the Phase
11.1 plan (session3.md L234489) and classifies the candidate's status per
subsystem with evidence:

  - oracle surface: functions/globals the current oracle exports in the
    subsystem. Membership is resolved from three evidence sources:
      1. the Doxygen public inventory (entity -> declaring header);
      2. the Clang AST atlas (header -> declarations);
      3. explicit symbol-prefix patterns for cross-cutting headers and for
         subsystems whose home header was removed (xmlBuffer*, xmlXLink*,
         SAX1, globals, threads) or which live in a separate DSO (EXSLT);
  - candidate surface: how many of those the candidate DSO exports;
  - obligations: the PARITY_OBLIGATIONS slice for the subsystem by
    implementation status, plus an explicit `obligation_gap` counting oracle
    members the obligations ledger does not cover (e.g. xmlBuffer*/xmlXLink*,
    which the ledger generator did not classify);
  - stubs and unknowns enumerated by name;
  - court coverage: differential court families exercising the subsystem;
  - verdict: PARITY_VERIFIED / IMPLEMENTED_UNVERIFIED / PARTIAL /
    STUB_ONLY / MISSING / UNOBLIGATED / HISTORICAL_ONLY / NO_CURRENT_SURFACE.

Historical-only subsystems (surface removed before the current oracle, e.g.
the DOCB parser family, buffer.h) are classified explicitly rather than
omitted. Every subsystem in the plan's enumeration receives a row.

Outputs:
    atlas/SUBSYSTEM_CENSUS.json   (canonical machine-readable ledger)
    atlas/SUBSYSTEM_CENSUS.md     (generated human-readable view)

Usage:
    python3 tools/evidence/subsystem_census.py
"""

import collections
import json
import os
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
API = {
    "libxml2": os.path.join(ROOT, "atlas", "api", "libxml2", "2.15.3.json"),
    "libxslt": os.path.join(ROOT, "atlas", "api", "libxslt", "1.1.45.json"),
}
SYMBOLS = {
    "libxml2": os.path.join(ROOT, "atlas", "symbols", "libxml2", "2.15.3.json"),
    "libxslt": os.path.join(ROOT, "atlas", "symbols", "libxslt", "1.1.45.json"),
}
DOXY = os.path.join(ROOT, "oracle", "historical", "doxygen",
                    "libxml2-system", "inventory-public.json")
OBLIGATIONS = os.path.join(ROOT, "atlas", "PARITY_OBLIGATIONS.json")
EXSLT_SO = "/usr/lib/libexslt.so.0"
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")
ORACLE_SO = {"libxml2": "/usr/lib/libxml2.so.16", "libxslt": "/usr/lib/libxslt.so.1"}
OUT_JSON = os.path.join(ROOT, "atlas", "SUBSYSTEM_CENSUS.json")
OUT_MD = os.path.join(ROOT, "atlas", "SUBSYSTEM_CENSUS.md")

# ── Taxonomy ─────────────────────────────────────────────────────────────
# headers:  declaring headers whose functions belong to the subsystem
# prefix:   symbol-prefix rule used instead of headers
# include:  symbol prefixes/local patterns pulled in from cross-cutting
#           headers (tree.h, parser.h, ...) or to refine membership
# exclude:  symbol prefixes kept out of a broad header assignment
# sources:  principal upstream implementation files (archaeology trees)
# plan:     the session3.md enumeration bullet(s) this subsystem satisfies
# note:     historical/provenance qualification

LIBXML2 = [
    dict(id="strings", title="Strings", plan="strings",
         headers=["xmlstring.h"], sources=["xmlstring.c"]),
    dict(id="memory", title="Memory allocation", plan="memory",
         headers=["xmlmemory.h"], sources=["xmlmemory.c"]),
    dict(id="dictionaries", title="Dictionaries", plan="dictionaries",
         headers=["dict.h"], sources=["dict.c"]),
    dict(id="hashes", title="Hash tables", plan="hashes",
         headers=["hash.h"], sources=["hash.c"]),
    dict(id="lists", title="Linked lists", plan="lists",
         headers=["list.h"], sources=["list.c"]),
    dict(id="buffers", title="Buffers", plan="buffers",
         prefix="xmlBuffer",
         sources=["buffer.c"],
         note="buffer.h was removed in libxml2 2.12; the xmlBuffer API "
              "survives deprecated in tree.h and remains exported."),
    dict(id="tree", title="Tree construction and navigation", plan="tree",
         headers=["tree.h"],
         exclude=["xmlBuffer", "xmlGetNs", "xmlNewNs", "xmlSearchNs",
                  "xmlSetNs", "xmlReconciliateNs", "xmlCopyNamespace",
                  "xmlFreeNs", "xmlNewGlobalNs", "xmlGetNsList",
                  "xmlAddDocEntity", "xmlAddDtdEntity", "xmlGetDocEntity",
                  "xmlGetDtdEntity", "xmlGetParameterEntity", "xmlNewEntity",
                  "xmlFreeEntity", "xmlEncodeEntities",
                  "xmlStringGetNodeList", "xmlStringLenGetNodeList",
                  "xmlNewEntityReference", "xmlCreateIntSubset",
                  "xmlGetIntSubset", "xmlNewDtd", "xmlFreeDtd",
                  "xmlNewEntityNode", "xmlAddEntity", "xmlGetDtdEntityDesc",
                  "xmlCreateElement", "xmlNewElementContent",
                  "xmlCopyElementContent", "xmlFreeElementContent",
                  "xmlCopyEnumeration", "xmlFreeEnumeration",
                  "xmlCreateEnumeration", "xmlAddElementDecl",
                  "xmlAddAttributeDecl", "xmlAddNotationDecl",
                  "xmlGetDtdElementDesc", "xmlGetDtdAttributeDesc",
                  "xmlGetDtdQAttrDesc", "xmlGetDtdNotationDesc",
                  "xmlCopyElement", "xmlCopyAttributeDecl", "xmlCopyNotation",
                  "xmlFreeElement", "xmlFreeAttribute", "xmlFreeNotation",
                  "xmlNodeDump", "xmlNodeDumpOutput", "xmlNodeDumpOutputInternal"],
         sources=["tree.c"]),
    dict(id="namespaces", title="Namespaces", plan="namespaces",
         headers=["tree.h"],
         include=["xmlGetNs", "xmlNewNs", "xmlSearchNs", "xmlSetNs",
                  "xmlReconciliateNs", "xmlCopyNamespace", "xmlFreeNs",
                  "xmlNewGlobalNs", "xmlGetNsList", "xmlNewNsProp",
                  "xmlSetNsProp", "xmlGetNsProp", "xmlHasNsProp",
                  "xmlCopyNamespaceList", "xmlFreeNsList"],
         sources=["tree.c", "parser.c"]),
    dict(id="entities", title="Entities", plan="entities",
         headers=["entities.h"],
         include=["xmlAddDocEntity", "xmlAddDtdEntity", "xmlGetDocEntity",
                  "xmlGetDtdEntity", "xmlGetParameterEntity", "xmlNewEntity",
                  "xmlFreeEntity", "xmlEncodeEntities", "xmlStringGetNodeList",
                  "xmlStringLenGetNodeList", "xmlNewEntityReference",
                  "xmlNewEntityNode", "xmlGetDtdEntityDesc",
                  "xmlStringDecodeEntities"],
         sources=["entities.c"]),
    dict(id="dtd", title="DTD (declarations and subsets)", plan="DTD",
         headers=["tree.h"],
         include=["xmlCreateIntSubset", "xmlGetIntSubset", "xmlNewDtd",
                  "xmlFreeDtd", "xmlAddElementDecl", "xmlAddAttributeDecl",
                  "xmlAddNotationDecl", "xmlGetDtdElementDesc",
                  "xmlGetDtdAttributeDesc", "xmlGetDtdQAttrDesc",
                  "xmlGetDtdNotationDesc", "xmlCopyElement",
                  "xmlCopyAttributeDecl", "xmlCopyNotation", "xmlFreeElement",
                  "xmlFreeAttribute", "xmlFreeNotation",
                  "xmlNewDocElementContent", "xmlNewElementContent",
                  "xmlCopyElementContent", "xmlFreeElementContent",
                  "xmlValidBuildContentModel", "xmlCopyEnumeration",
                  "xmlFreeEnumeration", "xmlCreateEnumeration"],
         sources=["valid.c", "tree.c"]),
    dict(id="validity", title="Validity (DTD validation)", plan="validity",
         headers=["valid.h"], sources=["valid.c"]),
    dict(id="xml-parser", title="XML parser", plan="XML parser; pull/document parser",
         headers=["parser.h"],
         exclude=["xmlCreatePushParserCtxt", "xmlCreateFileParserCtxt",
                  "xmlCreateMemoryParserCtxt", "xmlCreateIOParserCtxt",
                  "xmlCreateURLParserCtxt", "xmlCreateDocParserCtxt",
                  "xmlParserInputBufferCreate", "xmlParserGetDirectory",
                  "xmlParserInputBufferCreateMem",
                  "xmlParserInputBufferCreateFilename",
                  "xmlParserInputBufferCreateStatic", "xmlParserInputBufferPush",
                  "xmlParserInputBufferGrow", "xmlParserInputBufferRead",
                  "xmlParserInputBufferShrink", "xmlParserInputBufferFree",
                  "xmlNewIOInputStream", "xmlNewParserCtxt", "xmlFreeParserCtxt",
                  "xmlInitParserCtxt", "xmlSetupParserForBuffer",
                  "xmlCtxtResetPush", "xmlParseChunk",
                  "xmlParseDocument", "xmlParseMemory", "xmlParseFile",
                  "xmlParseBalancedChunkMemory", "xmlParseBalancedChunkMemoryRecover",
                  "xmlParseCtxtExternalEntity", "xmlParseExtParsedEnt",
                  "xmlParserHandleEntity", "xmlCreateEntityParserCtxt"],
         sources=["parser.c", "parserInternals.c"]),
    dict(id="parser-internals", title="Parser internals (output-affecting)",
         plan="parser internals that affect output",
         headers=["parserInternals.h"], sources=["parserInternals.c"]),
    dict(id="push-parser", title="Push parser and parser contexts",
         plan="push parser",
         headers=["parser.h"],
         include=["xmlCreatePushParserCtxt", "xmlCreateFileParserCtxt",
                  "xmlCreateMemoryParserCtxt", "xmlCreateIOParserCtxt",
                  "xmlCreateURLParserCtxt", "xmlCreateDocParserCtxt",
                  "xmlNewParserCtxt", "xmlFreeParserCtxt", "xmlInitParserCtxt",
                  "xmlSetupParserForBuffer", "xmlCtxtResetPush", "xmlParseChunk",
                  "xmlCreateEntityParserCtxt", "xmlParseDocument",
                  "xmlParseMemory", "xmlParseFile"],
         sources=["parser.c", "parserInternals.c"]),
    dict(id="sax1", title="SAX1", plan="SAX1",
         prefix="xmlSAX",
         include=["xmlSAXVersion", "xmlSAXParseDoc", "xmlSAXParseFile",
                  "xmlSAXParseMemory", "xmlSAXParseDTD", "xmlSAXParseEntity",
                  "xmlSAXUserParseFile", "xmlSAXUserParseMemory",
                  "xmlSAX2ParseDoc"],
         exclude=["xmlSAX2"],
         sources=["SAX.c", "parser.c"],
         note="SAX.h hosts the SAX1 handler struct; the SAX1 parse entry "
              "points are declared in parser.h. Membership is prefix-based."),
    dict(id="sax2", title="SAX2", plan="SAX2",
         headers=["SAX2.h"], sources=["SAX2.c"]),
    dict(id="html-parser", title="HTML parser", plan="HTML behavior actually targeted by upstream",
         headers=["HTMLparser.h"], sources=["HTMLparser.c"]),
    dict(id="html-tree", title="HTML tree and serialization", plan="HTML behavior",
         headers=["HTMLtree.h"], sources=["HTMLtree.c"]),
    dict(id="io", title="I/O (input/output buffers)", plan="I/O",
         headers=["xmlIO.h"],
         exclude=["xmlInputMatchCallback", "xmlInputOpenCallback",
                  "xmlInputReadCallback", "xmlInputCloseCallback",
                  "xmlOutputMatchCallback", "xmlOutputWriteCallback",
                  "xmlOutputCloseCallback", "xmlRegisterInputCallbacks",
                  "xmlRegisterOutputCallbacks",
                  "xmlRegisterDefaultInputCallbacks",
                  "xmlRegisterDefaultOutputCallbacks",
                  "xmlLoadExternalEntity", "xmlNoNetExternalEntityLoader",
                  "xmlGetExternalEntityLoader", "xmlSetExternalEntityLoader"],
         sources=["xmlIO.c"]),
    dict(id="resource-loading", title="Resource loading (entity loaders, nano FTP/HTTP)",
         plan="resource loading",
         headers=["nanoftp.h", "nanohttp.h"],
         include=["xmlLoadExternalEntity", "xmlNoNetExternalEntityLoader",
                  "xmlGetExternalEntityLoader", "xmlSetExternalEntityLoader",
                  "xmlInputMatchCallback", "xmlInputOpenCallback",
                  "xmlInputReadCallback", "xmlInputCloseCallback",
                  "xmlOutputMatchCallback", "xmlOutputWriteCallback",
                  "xmlOutputCloseCallback", "xmlRegisterInputCallbacks",
                  "xmlRegisterOutputCallbacks",
                  "xmlRegisterDefaultInputCallbacks",
                  "xmlRegisterDefaultOutputCallbacks"],
         sources=["xmlIO.c", "nanoftp.c", "nanohttp.c"]),
    dict(id="uri", title="URI handling", plan="URI",
         headers=["uri.h"], sources=["uri.c"]),
    dict(id="catalogs", title="Catalogs", plan="catalogs",
         headers=["catalog.h"], sources=["catalog.c"]),
    dict(id="encoding", title="Encoding (charsets, entities decoding)", plan="encoding",
         headers=["encoding.h", "chvalid.h", "xmlunicode.h"],
         sources=["encoding.c", "chvalid.c", "xmlunicode.c", "xmlchars.c"]),
    dict(id="serialization", title="Serialization (node/doc dump)", plan="serialization",
         headers=["xmlsave.h"],
         include=["xmlNodeDump", "xmlNodeDumpOutput", "xmlDocDump",
                  "xmlDocDumpMemory", "xmlDocDumpMemoryEnc", "xmlDocFormatDump",
                  "xmlDocFormatDumpEnc", "xmlDocDumpFormatMemory",
                  "xmlDocDumpFormatMemoryEnc", "xmlSaveToFd", "xmlSaveToFilename",
                  "xmlSaveToIO", "xmlSaveToBuffer", "xmlSaveFormatTo",
                  "xmlSaveFormatFile", "xmlSaveFormatFileEnc", "xmlSaveFile",
                  "xmlSaveFileEnc", "xmlSaveFileTo", "xmlSaveSetEscape",
                  "xmlSaveSetAttrEscape", "xmlSaveClose", "xmlSaveFlush",
                  "xmlSaveDoc", "xmlSaveTree"],
         sources=["xmlsave.c", "tree.c", "xmlIO.c"]),
    dict(id="debug-xml", title="Debug XML (shell/dump)", plan="debug XML",
         headers=["debugXML.h"], sources=["debugXML.c"]),
    dict(id="xpath", title="XPath", plan="XPath",
         headers=["xpath.h"], sources=["xpath.c"]),
    dict(id="xpath-internals", title="XPath internals (visible via API behavior)",
         plan="XPath internals visible through API behavior",
         headers=["xpathInternals.h"], sources=["xpath.c"]),
    dict(id="xpointer", title="XPointer", plan="XPointer",
         headers=["xpointer.h"], sources=["xpointer.c"]),
    dict(id="xinclude", title="XInclude", plan="XInclude",
         headers=["xinclude.h"], sources=["xinclude.c"]),
    dict(id="xlink", title="XLink", plan="(XLink supporting surface)",
         prefix="xlink", sources=["xlink.c"],
         note="xlink.h functions are exported but undocumented (absent from "
              "the Doxygen public inventory)."),
    dict(id="xsd", title="XML Schema (validators)", plan="XML Schema",
         headers=["xmlschemas.h", "schemasInternals.h"], sources=["xmlschemas.c"]),
    dict(id="xsd-datatypes", title="XML Schema datatypes", plan="XML Schema datatypes",
         headers=["xmlschemastypes.h"], sources=["xmlschemastypes.c"]),
    dict(id="relaxng", title="RELAX NG", plan="RELAX NG",
         headers=["relaxng.h"], sources=["relaxng.c"]),
    dict(id="schematron", title="Schematron", plan="Schematron",
         headers=["schematron.h"], sources=["schematron.c"]),
    dict(id="regex", title="Regex", plan="regex",
         headers=["xmlregexp.h"], sources=["xmlregexp.c"]),
    dict(id="automata", title="Automata", plan="automata",
         headers=["xmlautomata.h"], sources=["xmlautomata.c"]),
    dict(id="c14n", title="C14N (canonicalization)", plan="C14N; Exclusive C14N",
         headers=["c14n.h"], sources=["c14n.c"]),
    dict(id="reader", title="XML Reader", plan="XML Reader",
         headers=["xmlreader.h"], sources=["xmlreader.c"]),
    dict(id="writer", title="XML Writer", plan="XML Writer",
         headers=["xmlwriter.h"], sources=["xmlwriter.c"]),
    dict(id="globals", title="Globals (defaults, error context)", plan="globals",
         include=["xmlInitGlobals", "xmlCleanupGlobals",
                  "xmlGetWarningsDefaultValue", "xmlSetWarningsDefaultValue",
                  "xmlGetIndentTreeOutput", "xmlSetIndentTreeOutput",
                  "xmlGetTreeIndentString", "xmlSetTreeIndentString",
                  "xmlGetSaveNoEmptyTags", "xmlSetSaveNoEmptyTags",
                  "xmlGetKeepBlanksDefaultValue", "xmlSetKeepBlanksDefaultValue",
                  "xmlGetLineNumbersDefaultValue", "xmlSetLineNumbersDefaultValue",
                  "xmlGetLoadExternalEntityDefaultValue",
                  "xmlSetLoadExternalEntityDefaultValue",
                  "xmlGetSubstituteEntitiesDefaultValue",
                  "xmlSetSubstituteEntitiesDefaultValue",
                  "xmlGetPedanticParserDefaultValue",
                  "xmlSetPedanticParserDefaultValue",
                  "xmlGetDebugEntitiesDefaultValue",
                  "xmlSetDebugEntitiesDefaultValue",
                  "xmlGetDoValidityCheckingDefaultValue",
                  "xmlSetDoValidityCheckingDefaultValue",
                  "xmlGetGlobalState", "xmlGetThrDefKeepBlanksDefaultValue"],
         exclude=["xmlSAX", "xmlSAX2"],
         sources=["globals.c", "parserInternals.c"],
         note="The default-value accessors are declared across several "
              "headers (globals.h, parserInternals.h, tree.h); membership "
              "is by explicit name."),
    dict(id="threads", title="Threads", plan="threads",
         headers=["threads.h"], sources=["threads.c"]),
    dict(id="errors", title="Errors (structured/generic)", plan="errors",
         headers=["xmlerror.h"], sources=["error.c"]),
    dict(id="pattern", title="Pattern APIs (xmlPattern)", plan="pattern APIs",
         headers=["pattern.h"], sources=["pattern.c"]),
    dict(id="modules", title="Modules/plugins", plan="modules/plugins if present in applicable eras",
         headers=["xmlmodule.h"], sources=["xmlmodule.c"]),
    dict(id="legacy", title="Legacy/deprecated interfaces",
         plan="legacy/deprecated interfaces",
         headers=["tree.h", "parser.h", "parserInternals.h"],
         include=["xmlParserHandleEntity", "xmlAddEntity",
                  "xmlSetEntityReferenceFunc", "xmlParserPrintFileInfo",
                  "xmlParserPrintFileContext", "xmlGetCharEncodingName",
                  "xmlCreateEntityParserCtxt", "xmlParseEntity", "xmlParseDTD",
                  "xmlParseChunk", "xmlParseCtxtExternalEntity",
                  "xmlParseExtParsedEnt", "xmlParseQuarkSystemID",
                  "xmlParseQuarkPublicID", "xmlParseName", "xmlParseNmtoken",
                  "xmlParseEntityValue", "xmlParseAttValue",
                  "xmlParseSystemLiteral", "xmlParsePubidLiteral",
                  "xmlParseCharData", "xmlParseCDSect", "xmlParseComment",
                  "xmlParseProcessingInstruction", "xmlParseDocTypeDecl",
                  "xmlParseElementDecl", "xmlParseAttributeListDecl",
                  "xmlParseEntityDecl", "xmlParseNotationDecl",
                  "xmlParseElement", "xmlParseElementStart", "xmlParseElementEnd",
                  "xmlParseContent", "xmlParseReference", "xmlParseCharRef",
                  "xmlParseMisc", "xmlParseExternalSubset", "xmlParseMarkupDecl",
                  "xmlParseStartTag", "xmlParseEndTag",
                  "xmlParseInternalSubset", "xmlParseNotationType",
                  "xmlParseEnumerationType", "xmlParseAttribute",
                  "xmlParseEncodingDecl", "xmlParseVersionInfo",
                  "xmlParseXMLDecl", "xmlParseSDDecl", "xmlParsePEReference",
                  "xmlParserHandlePEReference", "xmlParserStringEncodings",
                  "xmlCurrentChar", "xmlCopyChar", "xmlCopyCharMultiByte",
                  "xmlParserInputGrow", "xmlParserInputShrink",
                  "xmlParserInputRead", "xmlParserInputUpdate",
                  "xmlParserHandleEntity", "xmlParseBalancedChunkMemory",
                  "xmlParseBalancedChunkMemoryRecover", "xmlParseDocument",
                  "xmlParseMemory", "xmlParseFile"],
         sources=["parser.c", "parserInternals.c", "entities.c", "tree.c"],
         note="Legacy 1.x/2.0-era entry points. The DOCB parser family "
              "(xmlDocb*, xmlParseDocTypeDecl-era DOCB paths) was removed "
              "from libxml2 in 2.12 and is historical surface only; it is "
              "tracked by the historical epoch atlases."),
    dict(id="feature-version", title="Feature/version APIs", plan="feature/version APIs",
         headers=["xmlversion.h", "xmlexports.h"],
         include=["xmlCheckVersion", "xmlParserVersion", "xmlGetVersion",
                  "xmlHasFeature"],
         sources=["xmlversion.c"]),
]

LIBXSLT = [
    dict(id="xslt-lifecycle", title="Stylesheet lifecycle", plan="stylesheet lifecycle",
         headers=["xslt.h", "xsltutils.h"],
         include=["xsltParseStylesheetFile", "xsltParseStylesheetProcess",
                  "xsltParseStylesheetInput", "xsltParseStylesheetDoc",
                  "xsltFreeStylesheet", "xsltNewStylesheet",
                  "xsltParseStylesheetComp", "xsltStylesheetComputeMaxDepth"],
         sources=["xslt.c", "xsltutils.c"]),
    dict(id="xslt-compilation", title="Compilation (preproc)", plan="compilation",
         headers=["preproc.h"], sources=["preproc.c"]),
    dict(id="xslt-imports", title="Imports", plan="imports",
         headers=["imports.h"], sources=["imports.c"]),
    dict(id="xslt-includes", title="Includes (document())", plan="includes",
         headers=["xsltutils.h", "transform.h"],
         include=["xsltLoadDocument", "xsltLoadStyleDocument",
                  "xsltDocumentFunction"],
         sources=["xslt.c"]),
    dict(id="xslt-templates", title="Templates", plan="templates",
         headers=["templates.h"], sources=["templates.c"]),
    dict(id="xslt-patterns", title="Patterns", plan="patterns",
         headers=["pattern.h"], sources=["pattern.c"]),
    dict(id="xslt-priorities", title="Priorities (template matching)", plan="priorities",
         headers=["xsltutils.h", "templates.h"],
         include=["xsltGetTemplate", "xsltFindTemplate"],
         sources=["templates.c", "xslt.c"]),
    dict(id="xslt-variables", title="Variables", plan="variables",
         headers=["variables.h"], sources=["variables.c"]),
    dict(id="xslt-parameters", title="Parameters", plan="parameters",
         headers=["xsltutils.h"],
         include=["xsltSetParam", "xsltParseStylesheetParam", "xsltAddParam",
                  "xsltFreeParam", "xsltEvalGlobalParams", "xsltRunStylesheetUser"],
         sources=["variables.c", "xsltutils.c"]),
    dict(id="xslt-rvt", title="Result tree fragments (RVTs)", plan="RVTs",
         headers=["xsltutils.h", "variables.h"],
         include=["xsltNewRVT", "xsltRegisterTmpRVT", "xsltFreeRVTs",
                  "xsltRegisterLocalRVT", "xsltCopyTextString",
                  "xsltCopyTreeList", "xsltStringToNodeSet",
                  "xsltNodeSetToString", "xsltFreeRVT"],
         sources=["variables.c", "xsltutils.c"]),
    dict(id="xslt-keys", title="Keys", plan="keys",
         headers=["keys.h"], sources=["keys.c"]),
    dict(id="xslt-sorting", title="Sorting", plan="sorting",
         headers=["xsltutils.h"],
         include=["xsltSort", "xsltComputeSortResult"],
         sources=["xslt.c", "xsltutils.c"]),
    dict(id="xslt-numbering", title="Numbering", plan="numbering",
         headers=["numbersInternals.h"],
         include=["xsltNumberFormat", "xsltNumber", "xsltFormatNumberConversion",
                  "xsltFormatNumberFunction", "xsltNumberFormatInsertNumbers",
                  "xsltNumberFormatGetAnyLevel", "xsltNumberFormatGetMultipleLevel",
                  "xsltNumberFormatGetSingleLevel", "xsltNumberFormatToPreparedFormat"],
         sources=["numbers.c"],
         note="numbersInternals.h has no Clang-AST function records; "
              "membership is by explicit name."),
    dict(id="xslt-whitespace", title="Whitespace (strip/preserve)", plan="whitespace",
         headers=["xsltutils.h"],
         include=["xsltApplyStripSpaces", "xsltSetStripSpaces",
                  "xsltSetPreserveSpace", "xsltGetStripSpace"],
         sources=["xslt.c"]),
    dict(id="xslt-namespace-alias", title="Namespace alias", plan="namespace alias",
         headers=["namespaces.h"],
         include=["xsltNamespaceAlias", "xsltCopyNamespace",
                  "xsltCopyNamespaceList", "xsltFreeNamespaceAliasHashes",
                  "xsltGetCNsProp", "xsltGetNamespace", "xsltGetNsProp",
                  "xsltGetPlainNamespace", "xsltGetSpecialNamespace"],
         sources=["namespaces.c"],
         note="namespaces.h has no Clang-AST function records; membership is "
              "by explicit name."),
    dict(id="xslt-attribute-sets", title="Attribute sets", plan="attribute sets",
         headers=["attributes.h"], sources=["attributes.c"]),
    dict(id="xslt-decimal-formats", title="Decimal formats", plan="decimal formats",
         headers=["xsltutils.h"],
         include=["xsltDecimalFormatGetByQName",
                  "xsltParseStylesheetDecimalFormat",
                  "xsltDecimalFormatCatQName", "xsltDecimalFormatGetByName"],
         sources=["xsltutils.c"]),
    dict(id="xslt-documents", title="Documents", plan="documents",
         headers=["documents.h"], sources=["documents.c"]),
    dict(id="xslt-transform-ctxt", title="Transformation contexts", plan="transformation contexts",
         headers=["xslt.h", "xsltutils.h"],
         include=["xsltNewTransformContext", "xsltFreeTransformContext",
                  "xsltApplyStylesheet", "xsltApplyStylesheetUser",
                  "xsltApplyStylesheetInternal", "xsltRunStylesheet",
                  "xsltRunStylesheetUser", "xsltNewSecurityPrefs",
                  "xsltFreeSecurityPrefs"],
         sources=["transform.c", "xslt.c"]),
    dict(id="xslt-transform-exec", title="Transform execution", plan="transform execution",
         headers=["transform.h"], sources=["transform.c"]),
    dict(id="xslt-extension-functions", title="Extension functions", plan="extension functions",
         headers=["functions.h", "extensions.h"], sources=["functions.c", "extensions.c"]),
    dict(id="xslt-extension-elements", title="Extension elements", plan="extension elements",
         headers=["extensions.h"], sources=["extensions.c"]),
    dict(id="xslt-security", title="Security preferences", plan="security preferences",
         headers=["security.h"], sources=["security.c"]),
    dict(id="xslt-output", title="Output/serialization", plan="output/serialization",
         headers=["xsltutils.h", "transform.h"],
         include=["xsltSaveResultTo", "xsltSaveResultToFilename",
                  "xsltSaveResultToFile", "xsltSaveResultToFd",
                  "xsltSaveResultToIO", "xsltSaveResultToBuffer",
                  "xsltSaveProfiling"],
         sources=["xsltutils.c", "transform.c"]),
    dict(id="xslt-profiling", title="Profiling", plan="profiling",
         headers=["xsltutils.h"],
         include=["xsltSaveProfiling", "xsltProfileStylesheet",
                  "xsltGetProfileInformation"],
         sources=["xsltutils.c"]),
    dict(id="xslt-errors", title="Errors", plan="errors",
         headers=["xsltutils.h"],
         include=["xsltSetGenericErrorFunc", "xsltSetTransformErrorFunc",
                  "xsltTransformError", "xsltPrintErrorContext",
                  "xsltSetCtxtParseOptions"],
         sources=["xsltutils.c", "transform.c"]),
    dict(id="xslt-global-state", title="Global state", plan="global state",
         headers=["xsltutils.h", "xslt.h"],
         include=["xsltGetMaxDepth", "xsltSetMaxDepth", "xsltGetMaxVars",
                  "xsltSetMaxVars", "xsltGetDefaultPriority",
                  "xsltSetDefaultPriority", "xsltGetDebugStatus",
                  "xsltSetDebugStatus", "xsltGetIndent", "xsltSetIndent",
                  "xsltGetSecurityPrefs", "xsltSetSecurityPrefs",
                  "xsltGetExtFunction", "xsltSetExtFunction",
                  "xsltGetExtElement", "xsltSetExtElement",
                  "xsltGetExtModuleFunction", "xsltSetExtModuleFunction",
                  "xsltGetExtModuleElement", "xsltSetExtModuleElement",
                  "xsltGetExtModuleTopLevel", "xsltSetExtModuleTopLevel",
                  "xsltRegisterExtModuleFunction",
                  "xsltRegisterExtModuleElement",
                  "xsltRegisterExtModuleTopLevel",
                  "xsltUnregisterExtModuleFunction",
                  "xsltUnregisterExtModuleElement",
                  "xsltUnregisterExtModuleTopLevel",
                  "xsltRegisterAllExtras"],
         sources=["xslt.c", "extensions.c", "xsltutils.c"]),
    dict(id="xslt-loader-hooks", title="Loader hooks (document loader)", plan="loader hooks",
         headers=["documents.h"], sources=["documents.c"]),
    dict(id="xslt-debugger", title="Debugger hooks", plan="debugger hooks where applicable",
         headers=["xsltutils.h"],
         include=["xsltSetDebuggerStatus", "xsltGetDebuggerStatus",
                  "xsltSetDebuggerCallbacks"],
         sources=["xsltutils.c"]),
    dict(id="xslt-extra", title="Extra/nonstandard extensions", plan="extra/nonstandard extensions",
         headers=["extra.h"], sources=["extra.c"]),
    dict(id="xslt-exports", title="Exported internals relied upon by consumers",
         plan="all exported internals historically relied upon by consumers",
         headers=["xsltInternals.h", "xsltconfig.h", "xsltlocale.h"],
         sources=["xslt.c", "xsltutils.c", "transform.c"]),
]

EXSLT = [
    dict(id="exslt-common", title="EXSLT common", plan="common",
         prefix="exsltCommon", sources=["common.c"]),
    dict(id="exslt-math", title="EXSLT math", plan="math",
         prefix="exsltMath", sources=["math.c"]),
    dict(id="exslt-sets", title="EXSLT sets", plan="sets",
         prefix="exsltSets", sources=["sets.c"]),
    dict(id="exslt-strings", title="EXSLT strings", plan="strings",
         prefix="exsltStr", sources=["strings.c"]),
    dict(id="exslt-dynamic", title="EXSLT dynamic", plan="dynamic",
         prefix="exsltDyn", sources=["dynamic.c"]),
    dict(id="exslt-dates", title="EXSLT dates", plan="dates",
         prefix="exsltDate", sources=["date.c"]),
    dict(id="exslt-functions", title="EXSLT functions", plan="functions",
         prefix="exsltFunc", sources=["functions.c"]),
    dict(id="exslt-registry", title="EXSLT registration surface",
         plan="all historically shipped modules and registrations",
         prefix="exslt", sources=["exslt.c"]),
]


def load(path):
    with open(path) as f:
        return json.load(f)


def nm_parts(so):
    """Split nm -D --defined-only output into (text/data) symbol names."""
    r = subprocess.run(["nm", "-D", "--defined-only", so],
                       capture_output=True, text=True)
    text, data = set(), set()
    for line in r.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[0] != "U":
            kind, name = parts[1], parts[-1]
            # Strip ELF symbol-versioning suffixes (libxslt uses @@LIBXML2_x).
            name = name.split("@", 1)[0]
            if kind in "TtWw":
                text.add(name)
            elif kind in "BbDdRr":
                data.add(name)
    return text, data


def nm_defined(so):
    text, data = nm_parts(so)
    return text | data


def build_member_index(project):
    """function name -> declaring header (Doxygen inventory ∪ Clang AST atlas)."""
    idx = {}
    api = load(API[project])
    for h in api["headers"]:
        hn = h.get("header")
        if not hn:
            continue
        for decl in h["declarations"]:
            if decl.get("kind") in ("FunctionDecl", "VarDecl") and decl.get("name"):
                idx.setdefault(decl["name"], set()).add(hn)
    if project == "libxml2":
        doxy = load(DOXY)
        for e in doxy["entities"]:
            if e.get("kind") == "function" and e.get("name") and e.get("header"):
                idx.setdefault(e["name"], set()).add(e["header"])
    return idx


def members_for(subsys, idx, oracle_fns, oracle_globals):
    members = set()
    for hn in subsys.get("headers", []):
        for name, hset in idx.items():
            if hn in hset:
                members.add(name)
    prefix = subsys.get("prefix")
    if prefix:
        for name in oracle_fns:
            if name.startswith(prefix):
                members.add(name)
        for name in oracle_globals:
            if name.startswith(prefix):
                members.add(name)
    for pat in subsys.get("include", []):
        members.add(pat)
    for pat in subsys.get("exclude", []):
        members.discard(pat)
    # keep only names that exist in the oracle surface
    return {m for m in members if m in oracle_fns or m in oracle_globals}


def subsystem_census():
    api = load(API["libxml2"])
    _ = api
    syms = {p: load(SYMBOLS[p]) for p in SYMBOLS}
    ob = load(OBLIGATIONS)
    cand_all = nm_defined(CAND_SO)
    cand_by_project = {"libxml2": cand_all, "libxslt": cand_all}
    exslt_oracle = sorted(nm_defined(EXSLT_SO))

    results = {}
    totals = collections.Counter()
    for project in ("libxml2", "libxslt"):
        tax = LIBXML2 if project == "libxml2" else LIBXSLT
        proj_ob = ob["projects"][project]
        by_symbol = {}
        for o in proj_ob["obligations"]:
            by_symbol.setdefault(o["oracle_symbol"], []).append(o)
        oracle_dso_fns, oracle_dso_globals = nm_parts(ORACLE_SO[project])
        clang_fns = set(syms[project]["functions"])
        clang_globals = set(syms[project].get("globals", []))
        oracle_fns = oracle_dso_fns | clang_fns
        oracle_globals = oracle_dso_globals | clang_globals
        candidate_fns = cand_by_project[project]
        idx = build_member_index(project)
        for sub in tax:
            members = members_for(sub, idx, oracle_fns, oracle_globals)
            m_fns = members & oracle_fns
            m_globals = members & oracle_globals
            dso_fns = m_fns & oracle_dso_fns
            dso_globals = m_globals & oracle_dso_globals
            cand = (m_fns | m_globals) & candidate_fns
            oblig = []
            for s in sorted(m_fns | m_globals):
                oblig.extend(by_symbol.get(s, []))
            gap = [s for s in sorted(m_fns | m_globals) if s not in by_symbol]
            # Real export gaps: oracle-DSO-exported members the candidate omits.
            missing = sorted((dso_fns | dso_globals) - cand)
            # Header-declared but not oracle-DSO-exported (informational).
            header_only = sorted(((m_fns | m_globals) - (dso_fns | dso_globals)))
            st = collections.Counter(o["implementation_status"] for o in oblig)
            sem_pass = sum(1 for o in oblig if o.get("semantic_status") == "PASS")
            courts = collections.Counter()
            for o in oblig:
                for c in o.get("courts", []):
                    courts[c] += 1
            if not m_fns and not m_globals:
                verdict = "HISTORICAL_ONLY" if sub.get("note") else "NO_CURRENT_SURFACE"
            elif st.get("STUB", 0) or st.get("UNKNOWN", 0) or missing:
                verdict = "PARTIAL"
            elif oblig and sem_pass == len(oblig):
                verdict = "PARITY_VERIFIED"
            elif not oblig:
                verdict = "UNOBLIGATED"
            elif not cand:
                verdict = "MISSING"
            else:
                verdict = "IMPLEMENTED_UNVERIFIED"
            results[sub["id"]] = dict(
                id=sub["id"], title=sub["title"], project=project, plan=sub["plan"],
                headers=sub.get("headers", []), prefix=sub.get("prefix", ""),
                sources=sub["sources"],
                oracle_dso_functions=len(dso_fns), oracle_dso_globals=len(dso_globals),
                header_only_functions=len(header_only),
                candidate_functions=len(cand),
                missing_symbols=missing,
                obligations_total=len(oblig),
                obligations=dict(st),
                obligation_gap=len(gap),
                semantic_verified=sem_pass,
                stub_symbols=sorted(o["oracle_symbol"] for o in oblig
                                    if o["implementation_status"] == "STUB"),
                unknown_symbols=sorted(o["oracle_symbol"] for o in oblig
                                       if o["implementation_status"] == "UNKNOWN"),
                courts=sorted(courts),
                verdict=verdict,
                note=sub.get("note", ""),
            )
            totals[verdict] += 1

    # EXSLT — membership from the oracle libexslt DSO by module prefix.
    exslt_dso_fns, _exslt_data = nm_parts(EXSLT_SO)
    proj_ob = ob["projects"]["libxslt"]
    by_symbol = {}
    for o in proj_ob["obligations"]:
        by_symbol.setdefault(o["oracle_symbol"], []).append(o)
    for sub in EXSLT:
        prefix = sub["prefix"]
        members = [f for f in exslt_dso_fns if f.startswith(prefix)]
        oblig = []
        for s in members:
            oblig.extend(by_symbol.get(s, []))
        gap = [s for s in members if s not in by_symbol]
        st = collections.Counter(o["implementation_status"] for o in oblig)
        sem_pass = sum(1 for o in oblig if o.get("semantic_status") == "PASS")
        cand = set(members) & cand_by_project["libxslt"]
        missing = sorted(set(members) - cand)
        if not members:
            verdict = "NO_CURRENT_SURFACE"
        elif st.get("STUB") or st.get("UNKNOWN") or missing:
            verdict = "PARTIAL"
        elif oblig and sem_pass == len(oblig):
            verdict = "PARITY_VERIFIED"
        elif not oblig:
            verdict = "UNOBLIGATED"
        elif not cand:
            verdict = "MISSING"
        else:
            verdict = "IMPLEMENTED_UNVERIFIED"
        results[sub["id"]] = dict(
            id=sub["id"], title=sub["title"], project="libexslt", plan=sub["plan"],
            headers=[], prefix=prefix, sources=sub["sources"],
            oracle_dso_functions=len(members), oracle_dso_globals=0,
            header_only_functions=0,
            candidate_functions=len(cand),
            missing_symbols=missing,
            obligations_total=len(oblig), obligations=dict(st),
            obligation_gap=len(gap), semantic_verified=sem_pass,
            stub_symbols=sorted(o["oracle_symbol"] for o in oblig
                                if o["implementation_status"] == "STUB"),
            unknown_symbols=sorted(o["oracle_symbol"] for o in oblig
                                   if o["implementation_status"] == "UNKNOWN"),
            courts=sorted({c for o in oblig for c in o.get("courts", [])}),
            verdict=verdict, note="")
        totals[verdict] += 1

    # Uncategorized oracle surface (must be small and accounted for).
    oracle_dso_fns, oracle_dso_globals = nm_parts(ORACLE_SO["libxml2"])
    all_members = set()
    idx = build_member_index("libxml2")
    for sub in LIBXML2:
        all_members |= members_for(sub, idx, oracle_dso_fns | set(syms["libxml2"]["functions"]),
                                   oracle_dso_globals | set(syms["libxml2"].get("globals", [])))
    uncat = [f for f in oracle_dso_fns if f not in all_members and not f.startswith("__")]
    out = {
        "schema": "subsystem-census-1",
        "generator": "tools/evidence/subsystem_census.py",
        "phase": "11.1-O",
        "projects": {"libxml2": "2.15.3", "libxslt": "1.1.45",
                     "libexslt": "1.1.45 (oracle libexslt.so.0)"},
        "exslt_oracle_symbols": exslt_oracle,
        "verdict_totals": dict(totals),
        "uncategorized_oracle_functions": sorted(uncat),
        "subsystems": results,
    }
    with open(OUT_JSON, "w") as f:
        json.dump(out, f, indent=1)
        f.write("\n")
    write_md(out)
    return out


def write_md(census):
    lines = []
    lines.append("# Subsystem Census — 11.1-O\n")
    lines.append("Generated by `tools/evidence/subsystem_census.py`. "
                 "Oracle: libxml2 2.15.3 / libxslt 1.1.45 / libexslt "
                 "(system DSOs).\n")
    lines.append("## Verdict totals\n")
    lines.append("| verdict | subsystems |")
    lines.append("|---|---|")
    for v, n in sorted(census["verdict_totals"].items()):
        lines.append(f"| {v} | {n} |")
    lines.append("")
    for project in ("libxml2", "libxslt", "libexslt"):
        subs = [s for s in census["subsystems"].values() if s["project"] == project]
        if not subs:
            continue
        lines.append(f"## {project}\n")
        lines.append("| subsystem | oracle DSO fn | cand fn | obligations | implemented | "
                     "stub | unknown | missing | sem-verified | verdict | courts |")
        lines.append("|---|---|---|---|---|---|---|---|---|---|---|")
        for s in sorted(subs, key=lambda x: x["id"]):
            lines.append(
                f"| {s['id']} | {s['oracle_dso_functions']} | {s['candidate_functions']} | "
                f"{s['obligations_total']} | {s['obligations'].get('IMPLEMENTED', 0)} | "
                f"{s['obligations'].get('STUB', 0)} | "
                f"{s['obligations'].get('UNKNOWN', 0)} | {len(s['missing_symbols'])} | "
                f"{s['semantic_verified']} | {s['verdict']} | {len(s['courts'])} |")
        lines.append("")
        lines.append("### Detail\n")
        for s in sorted(subs, key=lambda x: x["id"]):
            lines.append(f"#### {s['id']} — {s['title']}  ")
            lines.append(f"Plan item: *{s['plan']}*  ")
            hd = ", ".join(s["headers"]) if s["headers"] else (f"prefix `{s['prefix']}`" if s["prefix"] else "—")
            lines.append(f"Membership: {hd}  ")
            lines.append(f"Sources: {', '.join(s['sources'])}  ")
            lines.append(f"Oracle DSO functions {s['oracle_dso_functions']}, globals {s['oracle_dso_globals']}, "
                         f"header-declared-only {s['header_only_functions']}; "
                         f"candidate exports {s['candidate_functions']}; obligations {s['obligations_total']} "
                         f"({s['obligations'].get('IMPLEMENTED', 0)} implemented, "
                         f"{s['obligations'].get('STUB', 0)} stub, "
                         f"{s['obligations'].get('UNKNOWN', 0)} unknown, "
                         f"{s['obligations'].get('INTENTIONAL_NOOP', 0)} intentional noop, "
                         f"{s['obligations'].get('NOT_APPLICABLE', 0)} n/a); obligation gap "
                         f"{s['obligation_gap']}; semantic-verified {s['semantic_verified']}.  ")
            if s["stub_symbols"]:
                lines.append(f"Stubs: {', '.join(s['stub_symbols'])}  ")
            if s["unknown_symbols"]:
                lines.append(f"Unknowns: {', '.join(s['unknown_symbols'])}  ")
            if s.get("missing_symbols"):
                lines.append(f"Missing exports: {', '.join(s['missing_symbols'])}  ")
            lines.append(f"Verdict: **{s['verdict']}**. Courts: {', '.join(s['courts']) or '—'}")
            if s["note"]:
                lines.append(f"Note: {s['note']}")
            lines.append("")
    lines.append("## Uncategorized oracle functions\n")
    uncat = census["uncategorized_oracle_functions"]
    if uncat:
        lines.append(f"{len(uncat)}: " + ", ".join(uncat) + "\n")
    else:
        lines.append("0 — every oracle function is assigned to a subsystem.\n")
    with open(OUT_MD, "w") as f:
        f.write("\n".join(lines))


if __name__ == "__main__":
    census = subsystem_census()
    print(f"wrote {OUT_JSON}")
    print(f"wrote {OUT_MD}")
    print("verdict totals:", json.dumps(census["verdict_totals"]))
    print("uncategorized:", len(census["uncategorized_oracle_functions"]),
          census["uncategorized_oracle_functions"][:12])
