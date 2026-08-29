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
XMLPUBFUN const char *xmlOutputBufferGetContent(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferGetSize(xmlOutputBufferPtr out);
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
