/**
 * @file
 *
 * Regular expression API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_XMLREGEXP_H__
#define __XML_XMLREGEXP_H__

#include <libxml/xmlversion.h>
#include <stdio.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */




































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlRegExecCtxt xmlRegExecCtxt;

typedef struct _xmlRegexp xmlRegexp;

/* [11.1-G] end: extracted definitions */

/* [13.1] begin: regexp handle pointer typedefs (HOSTILE-FAILURE F8 closure)
 *
 * Phase 13 (HOSTILE-FAILURE F8): the oracle xmlregexp.h declares the
 * `xmlRegexpPtr`/`xmlRegExecCtxtPtr` handle typedefs, and hostile consumers
 * use them as return values of xmlRegexpCompile() & co; the candidate
 * exported the functions but the drop-in headers did not declare the
 * pointer aliases. Extracted verbatim from the upstream oracle header.
 */
typedef xmlRegexp *xmlRegexpPtr;

typedef xmlRegExecCtxt *xmlRegExecCtxtPtr;
/* [13.1] end: regexp handle pointer typedefs */

/* Callback type used by xmlRegNewExecCtxt (upstream xmlregexp.h) */
typedef void (*xmlRegExecCallbacks) (xmlRegExecCtxt *exec,
	                             const xmlChar *token,
				     void *transdata,
				     void *inputdata);

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlRegExecErrInfo (xmlRegExecCtxt *exec, const xmlChar **string, int *nbval, int *nbneg, xmlChar **values, int *terminal);
XMLPUBFUN int xmlRegExecNextValues(xmlRegExecCtxt *exec, int *nbval, int *nbneg, xmlChar **values, int *terminal);
XMLPUBFUN int xmlRegExecPushString(xmlRegExecCtxt *exec, const xmlChar *value, void *data);
XMLPUBFUN int xmlRegExecPushString2(xmlRegExecCtxt *exec, const xmlChar *value, const xmlChar *value2, void *data);
XMLPUBFUN void xmlRegFreeExecCtxt (xmlRegExecCtxt *exec);
XMLPUBFUN void xmlRegFreeRegexp(xmlRegexp *regexp);
XMLPUBFUN xmlRegExecCtxt * xmlRegNewExecCtxt (xmlRegexp *comp, xmlRegExecCallbacks callback, void *data);
XMLPUBFUN xmlRegexp * xmlRegexpCompile (const xmlChar *regexp);
XMLPUBFUN int xmlRegexpExec (xmlRegexp *comp, const xmlChar *value);
XMLPUBFUN int xmlRegexpIsDeterminist(xmlRegexp *comp);
XMLPUBFUN void xmlRegexpPrint (FILE *output, xmlRegexp *regexp);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLREGEXP_H__ */
