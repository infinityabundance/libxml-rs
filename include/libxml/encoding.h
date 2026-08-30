/**
 * @file
 *
 * Character encoding API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * Layout and constants match upstream `encoding.h` (libxml2 2.15.x).
 */

#ifndef __XML_ENCODING_H__
#define __XML_ENCODING_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlerror.h>
#include <libxml/xmlstring.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Encoding identifiers (upstream xmlCharEncoding) */
typedef enum {
    XML_CHAR_ENCODING_ERROR=   -1,
    XML_CHAR_ENCODING_NONE=	0,
    XML_CHAR_ENCODING_UTF8=	1,
    XML_CHAR_ENCODING_UTF16LE=	2,
    XML_CHAR_ENCODING_UTF16BE=	3,
    XML_CHAR_ENCODING_UCS4LE=	4,
    XML_CHAR_ENCODING_UCS4BE=	5,
    XML_CHAR_ENCODING_EBCDIC=	6,
    XML_CHAR_ENCODING_UCS4_2143=7,
    XML_CHAR_ENCODING_UCS4_3412=8,
    XML_CHAR_ENCODING_UCS2=	9,
    XML_CHAR_ENCODING_8859_1=	10,
    XML_CHAR_ENCODING_8859_2=	11,
    XML_CHAR_ENCODING_8859_3=	12,
    XML_CHAR_ENCODING_8859_4=	13,
    XML_CHAR_ENCODING_8859_5=	14,
    XML_CHAR_ENCODING_8859_6=	15,
    XML_CHAR_ENCODING_8859_7=	16,
    XML_CHAR_ENCODING_8859_8=	17,
    XML_CHAR_ENCODING_8859_9=	18,
    XML_CHAR_ENCODING_2022_JP=  19,
    XML_CHAR_ENCODING_SHIFT_JIS=20,
    XML_CHAR_ENCODING_EUC_JP=   21,
    XML_CHAR_ENCODING_ASCII=    22,
    XML_CHAR_ENCODING_UTF16=	23,
    XML_CHAR_ENCODING_HTML=	24,
    XML_CHAR_ENCODING_8859_10=	25,
    XML_CHAR_ENCODING_8859_11=	26,
    XML_CHAR_ENCODING_8859_13=	27,
    XML_CHAR_ENCODING_8859_14=	28,
    XML_CHAR_ENCODING_8859_15=	29,
    XML_CHAR_ENCODING_8859_16=	30,
    XML_CHAR_ENCODING_WINDOWS_1252 = 31
} xmlCharEncoding;

/* Conversion error codes (upstream xmlCharEncError) */
typedef enum {
    XML_ENC_ERR_SUCCESS     =  0,
    XML_ENC_ERR_INTERNAL    = -1,
    XML_ENC_ERR_INPUT       = -2,
    XML_ENC_ERR_SPACE       = -3,
    XML_ENC_ERR_MEMORY      = -4
} xmlCharEncError;

/* Encoding conversion flags (upstream xmlCharEncFlags) */
typedef enum {
    XML_ENC_INPUT = (1 << 0),
    XML_ENC_OUTPUT = (1 << 1),
    XML_ENC_HTML = (1 << 2)
} xmlCharEncFlags;

/* Conversion handler (upstream struct _xmlCharEncodingHandler layout) */
typedef struct _xmlCharEncodingHandler xmlCharEncodingHandler;
typedef xmlCharEncodingHandler *xmlCharEncodingHandlerPtr;

/* Conversion functions (upstream encoding.h) */
typedef int (*xmlCharEncodingInputFunc)(unsigned char *out, int *outlen,
                                         const unsigned char *in, int *inlen);
typedef int (*xmlCharEncodingOutputFunc)(unsigned char *out, int *outlen,
                                          const unsigned char *in, int *inlen);
typedef xmlCharEncError
(*xmlCharEncConvFunc)(void *vctxt, unsigned char *out, int *outlen,
                      const unsigned char *in, int *inlen, int flush);
typedef void
(*xmlCharEncConvCtxtDtor)(void *vctxt);
typedef xmlParserErrors
(*xmlCharEncConvImpl)(void *vctxt, const char *name, xmlCharEncFlags flags,
                      xmlCharEncodingHandler **out);

/* Conversion handler struct (upstream layout) */
struct _xmlCharEncodingHandler {
    char *name;
    union {
        xmlCharEncConvFunc func;
        xmlCharEncodingInputFunc legacyFunc;
    } input;
    union {
        xmlCharEncConvFunc func;
        xmlCharEncodingOutputFunc legacyFunc;
    } output;
    void *inputCtxt;
    void *outputCtxt;
    xmlCharEncConvCtxtDtor ctxtDtor;
    int flags;
};

XMLPUBFUN int xmlGetCharEncoding(const char *name);
XMLPUBFUN xmlCharEncodingHandlerPtr xmlFindCharEncodingHandler(const char *name);
XMLPUBFUN int xmlCharEncCloseFunc(xmlCharEncodingHandlerPtr handler);
XMLPUBFUN xmlParserErrors
    xmlLookupCharEncodingHandler(xmlCharEncoding enc,
                                xmlCharEncodingHandler **out);
XMLPUBFUN xmlParserErrors
    xmlOpenCharEncodingHandler(const char *name, int output,
                               xmlCharEncodingHandler **out);
XMLPUBFUN xmlParserErrors
    xmlCreateCharEncodingHandler(const char *name, xmlCharEncFlags flags,
                                 xmlCharEncConvImpl impl, void *implCtxt,
                                 xmlCharEncodingHandler **out);
XMLPUBFUN xmlCharEncodingHandler *
    xmlGetCharEncodingHandler(xmlCharEncoding enc);
XMLPUBFUN xmlParserErrors
    xmlCharEncNewCustomHandler(const char *name,
                               xmlCharEncConvFunc input,
                               xmlCharEncConvFunc output,
                               xmlCharEncConvCtxtDtor ctxtDtor,
                               void *inputCtxt, void *outputCtxt,
                               xmlCharEncodingHandler **out);

XMLPUBFUN void xmlInitCharEncodingHandlers(void);
XMLPUBFUN void xmlCleanupCharEncodingHandlers(void);
XMLPUBFUN void xmlRegisterCharEncodingHandler(xmlCharEncodingHandler *handler);
XMLPUBFUN xmlCharEncodingHandler *
    xmlNewCharEncodingHandler(const char *name,
                              xmlCharEncodingInputFunc input,
                              xmlCharEncodingOutputFunc output);
XMLPUBFUN int xmlAddEncodingAlias(const char *name, const char *alias);
XMLPUBFUN int xmlDelEncodingAlias(const char *alias);
XMLPUBFUN const char *xmlGetEncodingAlias(const char *alias);
XMLPUBFUN void xmlCleanupEncodingAliases(void);
XMLPUBFUN xmlCharEncoding xmlParseCharEncoding(const char *name);
XMLPUBFUN const char *xmlGetCharEncodingName(xmlCharEncoding enc);



#ifdef __cplusplus
}
#endif

#endif /* __XML_ENCODING_H__ */
