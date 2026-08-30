/**
 * @file
 *
 * Pattern API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_PATTERN_H__
#define __XML_PATTERN_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/dict.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

































/* Functions will be declared here as they are implemented. */

















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlPattern xmlPattern;

typedef struct _xmlStreamCtxt xmlStreamCtxt;

typedef enum{
    XML_PATTERN_DEFAULT		= 0,	/* simple pattern match */
    XML_PATTERN_XPATH		= 1<<0,	/* standard XPath pattern */
    XML_PATTERN_XSSEL		= 1<<1,	/* XPath subset for schema selector */
    XML_PATTERN_XSFIELD		= 1<<2	/* XPath subset for schema field */
} xmlPatternFlags;

/* [11.1-G] end: extracted definitions */

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN void xmlFreePattern (xmlPattern *comp);
XMLPUBFUN void xmlFreePatternList (xmlPattern *comp);
XMLPUBFUN void xmlFreeStreamCtxt (xmlStreamCtxt *stream);
XMLPUBFUN int xmlPatternCompileSafe (const xmlChar *pattern, xmlDict *dict, int flags, const xmlChar **namespaces, xmlPattern **patternOut);
XMLPUBFUN int xmlPatternFromRoot (xmlPattern *comp);
XMLPUBFUN xmlStreamCtxt * xmlPatternGetStreamCtxt (xmlPattern *comp);
XMLPUBFUN int xmlPatternMatch (xmlPattern *comp, xmlNode *node);
XMLPUBFUN int xmlPatternMaxDepth (xmlPattern *comp);
XMLPUBFUN int xmlPatternMinDepth (xmlPattern *comp);
XMLPUBFUN int xmlPatternStreamable (xmlPattern *comp);
XMLPUBFUN xmlPattern * xmlPatterncompile (const xmlChar *pattern, xmlDict *dict, int flags, const xmlChar **namespaces);
XMLPUBFUN int xmlStreamPop (xmlStreamCtxt *stream);
XMLPUBFUN int xmlStreamPush (xmlStreamCtxt *stream, const xmlChar *name, const xmlChar *ns);
XMLPUBFUN int xmlStreamPushAttr (xmlStreamCtxt *stream, const xmlChar *name, const xmlChar *ns);
XMLPUBFUN int xmlStreamPushNode (xmlStreamCtxt *stream, const xmlChar *name, const xmlChar *ns, int nodeType);
XMLPUBFUN int xmlStreamWantsAnyNode (xmlStreamCtxt *stream);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_PATTERN_H__ */
