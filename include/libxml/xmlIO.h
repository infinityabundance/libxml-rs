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

#ifdef __cplusplus
}
#endif

#endif /* __XML_IO_H__ */
