/**
 * @file
 *
 * Module API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_XMLMODULE_H__
#define __XML_XMLMODULE_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

































/* Functions will be declared here as they are implemented. */

















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlModule xmlModule;

typedef enum{
    XML_MODULE_LAZY = 1,	/* lazy binding */
    XML_MODULE_LOCAL= 2		/* local binding */
} xmlModuleOption;

/* [11.1-G] end: extracted definitions */

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlModuleClose (xmlModule *module);
XMLPUBFUN int xmlModuleFree (xmlModule *module);
XMLPUBFUN xmlModule *xmlModuleOpen (const char *filename, int options);
XMLPUBFUN int xmlModuleSymbol (xmlModule *module, const char* name, void **result);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLMODULE_H__ */
