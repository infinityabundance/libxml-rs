/**
 * @file ABI-STRUCT-NODE-0001-abicheck.c
 * @brief ABI probe: verify sizeof and offsetof for key libxml-rs structs.
 *
 * Court Casefile: ABI-STRUCT-NODE-0001
 * Description:   Structural ABI compliance check for libxml-rs.
 *                Compares struct sizes and field offsets against
 *                expected values to detect layout drift.
 *
 * Build:
 *   Oracle mode  (link system libxml2):
 *     gcc -o abicheck-oracle ABI-STRUCT-NODE-0001-abicheck.c \
 *         -lxml2 -lxslt
 *
 *   Candidate mode (our headers only, no link):
 *     gcc -fsyntax-only -c ABI-STRUCT-NODE-0001-abicheck.c \
 *         -I /path/to/include
 *
 * Usage:
 *   ./abicheck-oracle
 *
 * Output: Structured JSON-like lines with struct name, sizeof, and
 *         per-field offsetof values.  Return code 0 always (data
 *         collection, not assertion).
 */

#include <stddef.h>   /* offsetof */
#include <stdio.h>    /* printf  */
#include <libxml/tree.h>
#include <libxml/dict.h>
#include <libxml/hash.h>
#include <libxml/parser.h>
#include <libxml/xpath.h>

/* ------------------------------------------------------------------ */
/*  Helper: print a section header                                     */
/* ------------------------------------------------------------------ */
static void print_header(const char *label)
{
    printf("\n=== %s ===\n", label);
}

/* ------------------------------------------------------------------ */
/*  Helper: print sizeof for a type                                    */
/* ------------------------------------------------------------------ */
#define PRINT_SIZEOF(type_, label_)                                      \
    printf("  \"sizeof(%s)\": %zu,\n", (label_), sizeof(type_))

/* ------------------------------------------------------------------ */
/*  Helper: print offsetof for a field                                 */
/* ------------------------------------------------------------------ */
#define PRINT_OFFSETOF(type_, field_, label_)                            \
    printf("    \"offsetof(%s.%s)\": %zu,\n",                           \
           (label_), #field_, offsetof(type_, field_))

/* ------------------------------------------------------------------ */
/*  Check: _xmlNode                                                    */
/* ------------------------------------------------------------------ */
static void check_xmlNode(void)
{
    print_header("_xmlNode");
    PRINT_SIZEOF(struct _xmlNode, "_xmlNode");

    PRINT_OFFSETOF(struct _xmlNode, _private,  "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, type,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, name,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, children,  "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, last,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, parent,    "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, next,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, prev,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, doc,       "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, ns,        "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, content,   "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, properties,"_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, nsDef,     "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, psvi,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, line,      "_xmlNode");
    PRINT_OFFSETOF(struct _xmlNode, extra,     "_xmlNode");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlDoc                                                     */
/* ------------------------------------------------------------------ */
static void check_xmlDoc(void)
{
    print_header("_xmlDoc");
    PRINT_SIZEOF(struct _xmlDoc, "_xmlDoc");

    PRINT_OFFSETOF(struct _xmlDoc, _private,     "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, type,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, name,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, children,     "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, last,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, parent,       "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, next,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, prev,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, doc,          "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, compression,  "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, standalone,   "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, intSubset,    "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, extSubset,    "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, oldNs,        "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, version,      "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, encoding,     "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, ids,          "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, refs,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, URL,          "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, charset,      "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, dict,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, psvi,         "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, parseFlags,   "_xmlDoc");
    PRINT_OFFSETOF(struct _xmlDoc, properties,   "_xmlDoc");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlAttr                                                    */
/* ------------------------------------------------------------------ */
static void check_xmlAttr(void)
{
    print_header("_xmlAttr");
    PRINT_SIZEOF(struct _xmlAttr, "_xmlAttr");

    PRINT_OFFSETOF(struct _xmlAttr, _private, "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, type,     "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, name,     "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, children, "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, last,     "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, parent,   "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, next,     "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, prev,     "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, doc,      "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, ns,       "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, atype,    "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, psvi,     "_xmlAttr");
    PRINT_OFFSETOF(struct _xmlAttr, id,       "_xmlAttr");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlNs                                                      */
/* ------------------------------------------------------------------ */
static void check_xmlNs(void)
{
    print_header("_xmlNs");
    PRINT_SIZEOF(struct _xmlNs, "_xmlNs");

    PRINT_OFFSETOF(struct _xmlNs, next,     "_xmlNs");
    PRINT_OFFSETOF(struct _xmlNs, type,     "_xmlNs");
    PRINT_OFFSETOF(struct _xmlNs, href,     "_xmlNs");
    PRINT_OFFSETOF(struct _xmlNs, prefix,   "_xmlNs");
    PRINT_OFFSETOF(struct _xmlNs, _private, "_xmlNs");
    PRINT_OFFSETOF(struct _xmlNs, context,  "_xmlNs");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlDtd                                                     */
/* ------------------------------------------------------------------ */
static void check_xmlDtd(void)
{
    print_header("_xmlDtd");
    PRINT_SIZEOF(struct _xmlDtd, "_xmlDtd");

    PRINT_OFFSETOF(struct _xmlDtd, _private,    "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, type,        "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, name,        "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, children,    "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, last,        "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, parent,      "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, next,        "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, prev,        "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, doc,         "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, notations,   "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, elements,    "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, attributes,  "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, entities,    "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, ExternalID,  "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, SystemID,    "_xmlDtd");
    PRINT_OFFSETOF(struct _xmlDtd, pentities,   "_xmlDtd");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlEntity                                                  */
/* ------------------------------------------------------------------ */
static void check_xmlEntity(void)
{
    print_header("_xmlEntity");
    PRINT_SIZEOF(struct _xmlEntity, "_xmlEntity");

    PRINT_OFFSETOF(struct _xmlEntity, _private,   "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, type,       "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, name,       "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, children,   "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, last,       "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, parent,     "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, next,       "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, prev,       "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, doc,        "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, orig,       "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, content,    "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, length,     "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, etype,      "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, ExternalID, "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, SystemID,   "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, nexte,      "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, URI,        "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, owner,      "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, flags,      "_xmlEntity");
    PRINT_OFFSETOF(struct _xmlEntity, expandedSize, "_xmlEntity");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlBuffer                                                  */
/* ------------------------------------------------------------------ */
static void check_xmlBuffer(void)
{
    print_header("_xmlBuffer");
    PRINT_SIZEOF(struct _xmlBuffer, "_xmlBuffer");

    PRINT_OFFSETOF(struct _xmlBuffer, content,   "_xmlBuffer");
    PRINT_OFFSETOF(struct _xmlBuffer, use,       "_xmlBuffer");
    PRINT_OFFSETOF(struct _xmlBuffer, size,      "_xmlBuffer");
    PRINT_OFFSETOF(struct _xmlBuffer, alloc,     "_xmlBuffer");
    PRINT_OFFSETOF(struct _xmlBuffer, contentIO, "_xmlBuffer");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlError                                                   */
/* ------------------------------------------------------------------ */
static void check_xmlError(void)
{
    print_header("_xmlError");
    PRINT_SIZEOF(struct _xmlError, "_xmlError");

    PRINT_OFFSETOF(struct _xmlError, domain,  "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, code,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, message, "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, level,   "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, file,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, line,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, str1,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, str2,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, str3,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, int1,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, int2,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, ctxt,    "_xmlError");
    PRINT_OFFSETOF(struct _xmlError, node,    "_xmlError");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlParserCtxt                                              */
/* ------------------------------------------------------------------ */
static void check_xmlParserCtxt(void)
{
    print_header("_xmlParserCtxt");
    PRINT_SIZEOF(struct _xmlParserCtxt, "_xmlParserCtxt");

    PRINT_OFFSETOF(struct _xmlParserCtxt, sax,              "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, userData,         "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, myDoc,            "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, wellFormed,       "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, input,            "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, node,             "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, errNo,            "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, instate,          "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, dict,             "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, options,          "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, lastError,        "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, parseMode,        "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, depth,            "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, errorHandler,     "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, errorCtxt,        "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, resourceLoader,   "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, resourceCtxt,     "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, convImpl,         "_xmlParserCtxt");
    PRINT_OFFSETOF(struct _xmlParserCtxt, convCtxt,         "_xmlParserCtxt");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlXPathContext                                            */
/* ------------------------------------------------------------------ */
static void check_xmlXPathContext(void)
{
    print_header("_xmlXPathContext");
    PRINT_SIZEOF(struct _xmlXPathContext, "_xmlXPathContext");

    PRINT_OFFSETOF(struct _xmlXPathContext, doc,               "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, node,              "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, namespaces,        "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, nsNr,              "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, user,              "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, contextSize,       "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, proximityPosition, "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, varLookupFunc,     "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, funcLookupFunc,    "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, error,             "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, lastError,         "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, dict,              "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, flags,             "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, opLimit,           "_xmlXPathContext");
    PRINT_OFFSETOF(struct _xmlXPathContext, depth,             "_xmlXPathContext");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlXPathObject                                             */
/* ------------------------------------------------------------------ */
static void check_xmlXPathObject(void)
{
    print_header("_xmlXPathObject");
    PRINT_SIZEOF(struct _xmlXPathObject, "_xmlXPathObject");

    PRINT_OFFSETOF(struct _xmlXPathObject, type,       "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, nodesetval, "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, boolval,    "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, floatval,   "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, stringval,  "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, user,       "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, index,      "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, user2,      "_xmlXPathObject");
    PRINT_OFFSETOF(struct _xmlXPathObject, index2,     "_xmlXPathObject");
}

/* ------------------------------------------------------------------ */
/*  Check: _xmlNodeSet                                                 */
/* ------------------------------------------------------------------ */
static void check_xmlNodeSet(void)
{
    print_header("_xmlNodeSet");
    PRINT_SIZEOF(struct _xmlNodeSet, "_xmlNodeSet");

    PRINT_OFFSETOF(struct _xmlNodeSet, nodeNr,   "_xmlNodeSet");
    PRINT_OFFSETOF(struct _xmlNodeSet, nodeMax,  "_xmlNodeSet");
    PRINT_OFFSETOF(struct _xmlNodeSet, nodeTab,  "_xmlNodeSet");
}

/* ------------------------------------------------------------------ */
/*  main                                                               */
/* ------------------------------------------------------------------ */
int main(void)
{
    printf("{\n");
    printf("  \"probe\": \"ABI-STRUCT-NODE-0001\",\n");
    printf("  \"description\": \"Structural ABI compliance: sizeof and offsetof\",\n");
    printf("  \"library_version\": \"" LIBXML_DOTTED_VERSION "\",\n");
    printf("  \"results\": {\n");

    check_xmlNode();
    check_xmlDoc();
    check_xmlAttr();
    check_xmlNs();
    check_xmlDtd();
    check_xmlEntity();
    check_xmlBuffer();
    check_xmlError();
    check_xmlParserCtxt();
    check_xmlXPathContext();
    check_xmlXPathObject();
    check_xmlNodeSet();

    printf("  }\n");
    printf("}\n");

    return 0;
}
