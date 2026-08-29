/**
 * @file
 *
 * Save/serialization API (libxml-rs). Mirrors upstream libxml2 2.15.3
 * xmlsave.h.
 */

#ifndef __XML_XMLSAVE_H__
#define __XML_XMLSAVE_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/encoding.h>
#include <libxml/xmlIO.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    XML_SAVE_FORMAT = 1 << 0,
    XML_SAVE_NO_DECL = 1 << 1,
    XML_SAVE_NO_EMPTY = 1 << 2,
    XML_SAVE_NO_XHTML = 1 << 3,
    XML_SAVE_XHTML = 1 << 4,
    XML_SAVE_AS_XML = 1 << 5,
    XML_SAVE_AS_HTML = 1 << 6,
    XML_SAVE_WSNONSIG = 1 << 7,
    XML_SAVE_EMPTY = 1 << 8,
    XML_SAVE_NO_INDENT = 1 << 9,
    XML_SAVE_INDENT = 1 << 10
} xmlSaveOption;

typedef struct _xmlSaveCtxt xmlSaveCtxt;
typedef xmlSaveCtxt *xmlSaveCtxtPtr;

XMLPUBFUN xmlSaveCtxt *xmlSaveToFd(int fd, const char *encoding, int options);
XMLPUBFUN xmlSaveCtxt *xmlSaveToFilename(const char *filename,
                                          const char *encoding,
                                          int options);
XMLPUBFUN xmlSaveCtxt *xmlSaveToBuffer(xmlBuffer *buffer,
                                        const char *encoding,
                                        int options);
XMLPUBFUN xmlSaveCtxt *xmlSaveToIO(xmlOutputWriteCallback iowrite,
                                    xmlOutputCloseCallback ioclose,
                                    void *ioctx,
                                    const char *encoding,
                                    int options);
XMLPUBFUN long xmlSaveDoc(xmlSaveCtxt *ctxt, xmlDoc *doc);
XMLPUBFUN long xmlSaveTree(xmlSaveCtxt *ctxt, xmlNode *node);
XMLPUBFUN int xmlSaveFlush(xmlSaveCtxt *ctxt);
XMLPUBFUN int xmlSaveClose(xmlSaveCtxt *ctxt);
XMLPUBFUN xmlParserErrors xmlSaveFinish(xmlSaveCtxt *ctxt);
XMLPUBFUN int xmlSaveSetIndentString(xmlSaveCtxt *ctxt, const char *indent);
XMLPUBFUN int xmlSaveSetEscape(xmlSaveCtxt *ctxt, xmlCharEncodingOutputFunc escape);
XMLPUBFUN int xmlSaveSetAttrEscape(xmlSaveCtxt *ctxt, xmlCharEncodingOutputFunc escape);
XMLPUBFUN int xmlSaveFormatFileTo(xmlOutputBufferPtr buf, xmlDocPtr cur,
                                   const char *encoding, int format);
XMLPUBFUN int xmlSaveFileTo(xmlOutputBufferPtr buf, xmlDocPtr cur,
                             const char *encoding);
XMLPUBFUN int xmlSaveFormatFile(const char *filename, xmlDocPtr cur, int format);
XMLPUBFUN int xmlSaveFormatFileEnc(const char *filename, xmlDocPtr cur,
                                    const char *encoding, int format);

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLSAVE_H__ */
