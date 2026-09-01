/**
 * @file
 *
 * Parser internals for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_PARSER_INTERNALS_H__
#define __XML_PARSER_INTERNALS_H__

#include <libxml/xmlversion.h>
#include <libxml/parser.h>
#include <libxml/HTMLparser.h>
#include <libxml/chvalid.h>


#ifdef __cplusplus
extern "C" {
#endif

/* Global variables used for predefined strings (upstream parserInternals.h). */
XMLPUBVAR const xmlChar xmlStringText[];
XMLPUBVAR const xmlChar xmlStringTextNoenc[];
XML_DEPRECATED
XMLPUBVAR const xmlChar xmlStringComment[];

/* Deprecated character classification (parserInternals.h 2.15.3). */
XMLPUBFUN int xmlIsLetter(int c);

/* Upstream character-class macros (oracle parserInternals.h). */
#define IS_BYTE_CHAR(c)	 xmlIsChar_ch(c)
#define IS_CHAR(c)   xmlIsCharQ(c)
#define IS_CHAR_CH(c)  xmlIsChar_ch(c)
#define IS_BLANK(c)  xmlIsBlankQ(c)
#define IS_BLANK_CH(c)  xmlIsBlank_ch(c)
#define IS_BASECHAR(c) xmlIsBaseCharQ(c)
#define IS_DIGIT(c) xmlIsDigitQ(c)
#define IS_DIGIT_CH(c)  xmlIsDigit_ch(c)
#define IS_COMBINING(c) xmlIsCombiningQ(c)
#define IS_COMBINING_CH(c) 0
#define IS_EXTENDER(c) xmlIsExtenderQ(c)
#define IS_EXTENDER_CH(c)  xmlIsExtender_ch(c)
#define IS_IDEOGRAPHIC(c) xmlIsIdeographicQ(c)
#define IS_LETTER(c) (IS_BASECHAR(c) || IS_IDEOGRAPHIC(c))
#define IS_LETTER_CH(c) xmlIsBaseChar_ch(c)
#define IS_ASCII_LETTER(c)	((0x61 <= ((c) | 0x20)) && \
				 ((c) | 0x20) <= 0x7a)
#define IS_ASCII_DIGIT(c)	((0x30 <= (c)) && ((c) <= 0x39))
#define IS_PUBIDCHAR(c)	xmlIsPubidCharQ(c)
#define IS_PUBIDCHAR_CH(c) xmlIsPubidChar_ch(c)


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlParserInput * xmlCtxtPopInput (xmlParserCtxt *ctxt);
XMLPUBFUN int xmlCtxtPushInput (xmlParserCtxt *ctxt, xmlParserInput *input);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_PARSER_INTERNALS_H__ */

#ifdef __cplusplus
extern "C" {
#endif
/* [11.1-L] begin: callback-family declarations extracted verbatim
 * from the oracle libxml2 2.15.3 header (only symbols exported by
 * the candidate DSO are declared). */
XMLPUBFUN int			xmlCheckLanguageID	(const xmlChar *lang);

XMLPUBFUN int			xmlCopyChar		(int len,
						 xmlChar *out,
						 int val);

XMLPUBFUN int		xmlCopyCharMultiByte	(xmlChar *out,
						 int val);

XMLPUBFUN xmlParserCtxt *
			xmlCreateEntityParserCtxt(const xmlChar *URL,
						 const xmlChar *ID,
						 const xmlChar *base);

XMLPUBFUN xmlParserCtxt *
			xmlCreateFileParserCtxt	(const char *filename);

XMLPUBFUN xmlParserCtxt *
			xmlCreateMemoryParserCtxt(const char *buffer,
						 int size);

XMLPUBFUN xmlParserCtxt *
			xmlCreateURLParserCtxt	(const char *filename,
						 int options);

XMLPUBFUN void
			xmlCtxtErrMemory	(xmlParserCtxt *ctxt);

XMLPUBFUN int			xmlCurrentChar		(xmlParserCtxt *ctxt,
						 int *len);

XMLPUBFUN void
			xmlFreeInputStream	(xmlParserInput *input);

XMLPUBFUN xmlParserInput *
			xmlNewEntityInputStream	(xmlParserCtxt *ctxt,
						 xmlEntity *entity);

XMLPUBFUN xmlParserInput *
			xmlNewInputFromFile	(xmlParserCtxt *ctxt,
						 const char *filename);

XMLPUBFUN xmlParserInput *
			xmlNewInputStream	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlParserInput *
			xmlNewStringInputStream	(xmlParserCtxt *ctxt,
						 const xmlChar *buffer);

XMLPUBFUN void			xmlNextChar		(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseAttValue	(xmlParserCtxt *ctxt);

XMLPUBFUN const xmlChar *
			xmlParseAttribute	(xmlParserCtxt *ctxt,
						 xmlChar **value);

XMLPUBFUN void
			xmlParseAttributeListDecl(xmlParserCtxt *ctxt);

XMLPUBFUN int
			xmlParseAttributeType	(xmlParserCtxt *ctxt,
						 xmlEnumeration **tree);

XMLPUBFUN void
			xmlParseCDSect		(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseCharData	(xmlParserCtxt *ctxt,
						 int cdata);

XMLPUBFUN int
			xmlParseCharRef		(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseComment		(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseContent		(xmlParserCtxt *ctxt);

XMLPUBFUN int
			xmlParseDefaultDecl	(xmlParserCtxt *ctxt,
						 xmlChar **value);

XMLPUBFUN void
			xmlParseDocTypeDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseElement		(xmlParserCtxt *ctxt);

XMLPUBFUN xmlElementContent *
			xmlParseElementChildrenContentDecl
						(xmlParserCtxt *ctxt,
						 int inputchk);

XMLPUBFUN int
			xmlParseElementContentDecl(xmlParserCtxt *ctxt,
						 const xmlChar *name,
						 xmlElementContent **result);

XMLPUBFUN int
			xmlParseElementDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlElementContent *
			xmlParseElementMixedContentDecl
						(xmlParserCtxt *ctxt,
						 int inputchk);

XMLPUBFUN xmlChar *
			xmlParseEncName		(xmlParserCtxt *ctxt);

XMLPUBFUN const xmlChar *
			xmlParseEncodingDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseEndTag		(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseEntityDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlEntity *
			xmlParseEntityRef	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseEntityValue	(xmlParserCtxt *ctxt,
						 xmlChar **orig);

XMLPUBFUN int
			xmlParseEnumeratedType	(xmlParserCtxt *ctxt,
						 xmlEnumeration **tree);

XMLPUBFUN xmlEnumeration *
			xmlParseEnumerationType	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseExternalID	(xmlParserCtxt *ctxt,
						 xmlChar **publicId,
						 int strict);

XMLPUBFUN void
			xmlParseExternalSubset	(xmlParserCtxt *ctxt,
						 const xmlChar *publicId,
						 const xmlChar *systemId);

XMLPUBFUN void
			xmlParseMarkupDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseMisc		(xmlParserCtxt *ctxt);

XMLPUBFUN const xmlChar *
			xmlParseName		(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseNmtoken		(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseNotationDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlEnumeration *
			xmlParseNotationType	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParsePEReference	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParsePI		(xmlParserCtxt *ctxt);

XMLPUBFUN const xmlChar *
			xmlParsePITarget	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParsePubidLiteral	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseReference	(xmlParserCtxt *ctxt);

XMLPUBFUN int
			xmlParseSDDecl		(xmlParserCtxt *ctxt);

XMLPUBFUN const xmlChar *
			xmlParseStartTag	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseSystemLiteral	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseTextDecl	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseVersionInfo	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlParseVersionNum	(xmlParserCtxt *ctxt);

XMLPUBFUN void
			xmlParseXMLDecl		(xmlParserCtxt *ctxt);

XMLPUBFUN void			xmlParserHandlePEReference(xmlParserCtxt *ctxt);

XMLPUBFUN void			xmlParserInputShrink	(xmlParserInput *in);

XMLPUBFUN xmlChar
			xmlPopInput		(xmlParserCtxt *ctxt);

XMLPUBFUN int
			xmlPushInput		(xmlParserCtxt *ctxt,
						 xmlParserInput *input);

XMLPUBFUN int			xmlSkipBlankChars	(xmlParserCtxt *ctxt);

XMLPUBFUN xmlChar *
			xmlSplitQName		(xmlParserCtxt *ctxt,
						 const xmlChar *name,
						 xmlChar **prefix);

XMLPUBFUN int			xmlStringCurrentChar	(xmlParserCtxt *ctxt,
						 const xmlChar *cur,
						 int *len);

XMLPUBFUN xmlChar *
		xmlStringDecodeEntities		(xmlParserCtxt *ctxt,
						 const xmlChar *str,
						 int what,
						 xmlChar end,
						 xmlChar  end2,
						 xmlChar end3);

XMLPUBFUN xmlChar *
		xmlStringLenDecodeEntities	(xmlParserCtxt *ctxt,
						 const xmlChar *str,
						 int len,
						 int what,
						 xmlChar end,
						 xmlChar  end2,
						 xmlChar end3);

XMLPUBFUN int
			xmlSwitchEncoding	(xmlParserCtxt *ctxt,
						 xmlCharEncoding enc);

XMLPUBFUN int
			xmlSwitchEncodingName	(xmlParserCtxt *ctxt,
						 const char *encoding);

XMLPUBFUN int
			xmlSwitchInputEncoding	(xmlParserCtxt *ctxt,
						 xmlParserInput *input,
					 xmlCharEncodingHandler *handler);

XMLPUBFUN int
			xmlSwitchToEncoding	(xmlParserCtxt *ctxt,
					 xmlCharEncodingHandler *handler);

/* [11.1-L] end: extracted declarations */
#ifdef __cplusplus
}
#endif

