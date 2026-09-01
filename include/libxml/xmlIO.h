/**
 * @file
 *
 * I/O API for libxml-rs
 */

#ifndef __XML_IO_H__
#define __XML_IO_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/encoding.h>

#ifdef __cplusplus
extern "C" {
#endif

/* I/O callback types */
typedef int (*xmlInputReadCallback)(void *context, char *buffer, int len);
typedef int (*xmlInputCloseCallback)(void *context);
typedef int (*xmlOutputWriteCallback)(void *context, const char *buffer, int len);
typedef int (*xmlOutputCloseCallback)(void *context);
typedef int (*xmlInputMatchCallback) (const char *filename);
typedef void * (*xmlInputOpenCallback) (const char *filename);
typedef int (*xmlOutputMatchCallback) (const char *filename);
typedef void * (*xmlOutputOpenCallback) (const char *filename);

typedef xmlParserInputBuffer *
(*xmlParserInputBufferCreateFilenameFunc)(const char *URI, xmlCharEncoding enc);

/* Output buffers (xmlIO.h 2.15.3). tree.h already provides the opaque
 * `xmlOutputBuffer` / `xmlOutputBufferPtr` typedefs. */

XMLPUBFUN xmlOutputBufferPtr xmlAllocOutputBuffer(xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateBuffer(xmlBufferPtr buffer,
                                                         xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFile(FILE *file,
                                                       xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFilename(const char *URI,
                                                           xmlCharEncodingHandlerPtr encoder,
                                                           int compression);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFd(int fd,
                                                     xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateIO(xmlOutputWriteCallback iowrite,
                                                     xmlOutputCloseCallback ioclose,
                                                     void *ioctx,
                                                     xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN int xmlOutputBufferClose(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferFlush(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferWrite(xmlOutputBufferPtr out, int len, const char *data);
XMLPUBFUN int xmlOutputBufferWriteString(xmlOutputBufferPtr out, const char *str);
XMLPUBFUN int xmlOutputBufferWriteEscape(xmlOutputBufferPtr out,
                                          const xmlChar *str,
                                          xmlCharEncodingOutputFunc escaping);
XMLPUBFUN const xmlChar *xmlOutputBufferGetContent(xmlOutputBufferPtr out);
XMLPUBFUN size_t xmlOutputBufferGetSize(xmlOutputBufferPtr out);
typedef xmlOutputBufferPtr
(*xmlOutputBufferCreateFilenameFunc)(const char *URI,
                                     xmlCharEncodingHandlerPtr encoder,
                                     int compression);
XMLPUBFUN xmlOutputBufferCreateFilenameFunc
xmlOutputBufferCreateFilenameDefault(xmlOutputBufferCreateFilenameFunc func);

#ifdef __cplusplus
}
#endif

#endif /* __XML_IO_H__ */

#ifdef __cplusplus
extern "C" {
#endif
/* [11.1-L] begin: callback-family declarations extracted verbatim
 * from the oracle libxml2 2.15.3 header (only symbols exported by
 * the candidate DSO are declared). */
XMLPUBFUN xmlParserInputBuffer *
	xmlAllocParserInputBuffer		(xmlCharEncoding enc);

XMLPUBFUN int
	xmlCheckFilename		(const char *path);

XMLPUBFUN xmlParserInput *
	xmlCheckHTTPInput		(xmlParserCtxt *ctxt,
					 xmlParserInput *ret);

XMLPUBFUN void
	xmlCleanupInputCallbacks		(void);

XMLPUBFUN void
	xmlCleanupOutputCallbacks		(void);

XMLPUBFUN int
	xmlFileClose			(void * context);

XMLPUBFUN int
	xmlFileMatch			(const char *filename);

XMLPUBFUN void *
	xmlFileOpen			(const char *filename);

XMLPUBFUN int
	xmlFileRead			(void * context,
					 char * buffer,
					 int len);

XMLPUBFUN void
	xmlFreeParserInputBuffer		(xmlParserInputBuffer *in);

XMLPUBFUN int
	xmlIOHTTPClose			(void * context);

XMLPUBFUN int
	xmlIOHTTPMatch			(const char *filename);

XMLPUBFUN void *
	xmlIOHTTPOpen			(const char *filename);

XMLPUBFUN void *
	xmlIOHTTPOpenW			(const char * post_uri,
					 int   compression );

XMLPUBFUN int
	xmlIOHTTPRead			(void * context,
					 char * buffer,
					 int len);

XMLPUBFUN xmlParserInput *
	xmlNoNetExternalEntityLoader	(const char *URL,
					 const char *ID,
					 xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
	xmlNormalizeWindowsPath		(const xmlChar *path);

XMLPUBFUN char *
	xmlParserGetDirectory			(const char *filename);

XMLPUBFUN xmlParserInputBuffer *
	xmlParserInputBufferCreateFd		(int fd,
	                                         xmlCharEncoding enc);

XMLPUBFUN xmlParserInputBuffer *
	xmlParserInputBufferCreateFile		(FILE *file,
                                                 xmlCharEncoding enc);

XMLPUBFUN xmlParserInputBuffer *
	xmlParserInputBufferCreateFilename	(const char *URI,
                                                 xmlCharEncoding enc);

XMLPUBFUN xmlParserInputBufferCreateFilenameFunc
	xmlParserInputBufferCreateFilenameDefault(
		xmlParserInputBufferCreateFilenameFunc func);

XMLPUBFUN xmlParserInputBuffer *
	xmlParserInputBufferCreateIO		(xmlInputReadCallback   ioread,
						 xmlInputCloseCallback  ioclose,
						 void *ioctx,
	                                         xmlCharEncoding enc);

XMLPUBFUN xmlParserInputBuffer *
	xmlParserInputBufferCreateMem		(const char *mem, int size,
	                                         xmlCharEncoding enc);

XMLPUBFUN xmlParserInputBuffer *
	xmlParserInputBufferCreateStatic	(const char *mem, int size,
	                                         xmlCharEncoding enc);

XMLPUBFUN int
	xmlParserInputBufferGrow		(xmlParserInputBuffer *in,
						 int len);

XMLPUBFUN int
	xmlParserInputBufferPush		(xmlParserInputBuffer *in,
						 int len,
						 const char *buf);

XMLPUBFUN int
	xmlParserInputBufferRead		(xmlParserInputBuffer *in,
						 int len);

XMLPUBFUN int
	xmlPopInputCallbacks			(void);

XMLPUBFUN int
	xmlPopOutputCallbacks			(void);

XMLPUBFUN void
	xmlRegisterDefaultInputCallbacks	(void);

XMLPUBFUN void
	xmlRegisterDefaultOutputCallbacks(void);

XMLPUBFUN void
	xmlRegisterHTTPPostCallbacks	(void );

XMLPUBFUN int
	xmlRegisterInputCallbacks		(xmlInputMatchCallback matchFunc,
						 xmlInputOpenCallback openFunc,
						 xmlInputReadCallback readFunc,
						 xmlInputCloseCallback closeFunc);

XMLPUBFUN int
	xmlRegisterOutputCallbacks	(xmlOutputMatchCallback matchFunc,
					 xmlOutputOpenCallback openFunc,
					 xmlOutputWriteCallback writeFunc,
					 xmlOutputCloseCallback closeFunc);

XMLPUBFUN xmlOutputBufferCreateFilenameFunc
	xmlThrDefOutputBufferCreateFilenameDefault(
		xmlOutputBufferCreateFilenameFunc func);

XMLPUBFUN xmlParserInputBufferCreateFilenameFunc
	xmlThrDefParserInputBufferCreateFilenameDefault(
		xmlParserInputBufferCreateFilenameFunc func);

/* [13.1] begin: thread-local IO-hook accessors + macro aliases
 *
 * Phase 13 (HOSTILE-THREADS): upstream 2.15 keeps the input/output
 * create-filename callbacks per-thread (globals.c xmlGetThreadLocalStorage);
 * the oracle headers alias the names to `(*__xmlXxx())` accessor functions
 * and the oracle DSO exports only the accessors. The candidate implements
 * the same contract (src/xml/globals/tls.rs). Guarded by
 * XML_GLOBALS_NO_REDEFINITION exactly like upstream xmlIO.h.
 */
XMLPUBFUN xmlParserInputBufferCreateFilenameFunc *
	__xmlParserInputBufferCreateFilenameValue(void);
XMLPUBFUN xmlOutputBufferCreateFilenameFunc *
	__xmlOutputBufferCreateFilenameValue(void);

#ifndef XML_GLOBALS_NO_REDEFINITION
  #define xmlParserInputBufferCreateFilenameValue \
    (*__xmlParserInputBufferCreateFilenameValue())
  #define xmlOutputBufferCreateFilenameValue \
    (*__xmlOutputBufferCreateFilenameValue())
#endif /* XML_GLOBALS_NO_REDEFINITION */
/* [13.1] end: thread-local IO-hook accessors + macro aliases */

/* [11.1-L] end: extracted declarations */
#ifdef __cplusplus
}
#endif

