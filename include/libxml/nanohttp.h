/**
 * @file
 *
 * HTTP stubs API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_NANOHTTP_H__
#define __XML_NANOHTTP_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN const char * xmlNanoHTTPAuthHeader (void *ctx);
XMLPUBFUN void xmlNanoHTTPCleanup (void);
XMLPUBFUN void xmlNanoHTTPClose (void *ctx);
XMLPUBFUN int xmlNanoHTTPContentLength( void * ctx );
XMLPUBFUN const char * xmlNanoHTTPEncoding (void *ctx);
XMLPUBFUN int xmlNanoHTTPFetch (const char *URL, const char *filename, char **contentType);
XMLPUBFUN void xmlNanoHTTPInit (void);
XMLPUBFUN void * xmlNanoHTTPMethod (const char *URL, const char *method, const char *input, char **contentType, const char *headers, int ilen);
XMLPUBFUN void * xmlNanoHTTPMethodRedir (const char *URL, const char *method, const char *input, char **contentType, char **redir, const char *headers, int ilen);
XMLPUBFUN const char * xmlNanoHTTPMimeType (void *ctx);
XMLPUBFUN void * xmlNanoHTTPOpen (const char *URL, char **contentType);
XMLPUBFUN void * xmlNanoHTTPOpenRedir (const char *URL, char **contentType, char **redir);
XMLPUBFUN int xmlNanoHTTPRead (void *ctx, void *dest, int len);
XMLPUBFUN const char * xmlNanoHTTPRedir (void *ctx);
XMLPUBFUN int xmlNanoHTTPReturnCode (void *ctx);
XMLPUBFUN int xmlNanoHTTPSave (void *ctxt, const char *filename);
XMLPUBFUN void xmlNanoHTTPScanProxy (const char *URL);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_NANOHTTP_H__ */
