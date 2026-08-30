/**
 * @file
 *
 * HTML serializer API for libxml-rs
 */

#ifndef __HTML_TREE_H__
#define __HTML_TREE_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* HTML serializer functions - to be implemented */


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN void htmlDocContentDumpFormatOutput(xmlOutputBuffer *buf, xmlDoc *cur, const char *encoding, int format);
XMLPUBFUN void htmlDocContentDumpOutput(xmlOutputBuffer *buf, xmlDoc *cur, const char *encoding);
XMLPUBFUN int htmlDocDump (FILE *f, xmlDoc *cur);
XMLPUBFUN void htmlDocDumpMemory (xmlDoc *cur, xmlChar **mem, int *size);
XMLPUBFUN void htmlDocDumpMemoryFormat (xmlDoc *cur, xmlChar **mem, int *size, int format);
XMLPUBFUN const xmlChar * htmlGetMetaEncoding (xmlDoc *doc);
XMLPUBFUN int htmlIsBooleanAttr (const xmlChar *name);
XMLPUBFUN xmlDoc * htmlNewDoc (const xmlChar *URI, const xmlChar *ExternalID);
XMLPUBFUN xmlDoc * htmlNewDocNoDtD (const xmlChar *URI, const xmlChar *ExternalID);
XMLPUBFUN int htmlNodeDump (xmlBuffer *buf, xmlDoc *doc, xmlNode *cur);
XMLPUBFUN void htmlNodeDumpFile (FILE *out, xmlDoc *doc, xmlNode *cur);
XMLPUBFUN int htmlNodeDumpFileFormat (FILE *out, xmlDoc *doc, xmlNode *cur, const char *encoding, int format);
XMLPUBFUN void htmlNodeDumpFormatOutput(xmlOutputBuffer *buf, xmlDoc *doc, xmlNode *cur, const char *encoding, int format);
XMLPUBFUN void htmlNodeDumpOutput (xmlOutputBuffer *buf, xmlDoc *doc, xmlNode *cur, const char *encoding);
XMLPUBFUN int htmlSaveFile (const char *filename, xmlDoc *cur);
XMLPUBFUN int htmlSaveFileEnc (const char *filename, xmlDoc *cur, const char *encoding);
XMLPUBFUN int htmlSaveFileFormat (const char *filename, xmlDoc *cur, const char *encoding, int format);
XMLPUBFUN int htmlSetMetaEncoding (xmlDoc *doc, const xmlChar *encoding);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __HTML_TREE_H__ */
