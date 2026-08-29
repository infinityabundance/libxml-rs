/**
 * @file
 *
 * XLink API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * Layout and declarations mirror upstream `xlink.h` (libxml2 2.15.x):
 * callback typedefs, `xlinkType`/`xlinkShow`/`xlinkActuate` enums and the
 * `_xlinkHandler` callback set (3 function pointers, 24 bytes on LP64).
 */

#ifndef __XML_XLINK_H__
#define __XML_XLINK_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef xmlChar *xlinkHRef;
typedef xmlChar *xlinkRole;
typedef xmlChar *xlinkTitle;

typedef enum {
    XLINK_TYPE_NONE = 0,
    XLINK_TYPE_SIMPLE,
    XLINK_TYPE_EXTENDED,
    XLINK_TYPE_EXTENDED_SET
} xlinkType;

typedef enum {
    XLINK_SHOW_NONE = 0,
    XLINK_SHOW_NEW,
    XLINK_SHOW_EMBED,
    XLINK_SHOW_REPLACE
} xlinkShow;

typedef enum {
    XLINK_ACTUATE_NONE = 0,
    XLINK_ACTUATE_AUTO,
    XLINK_ACTUATE_ONREQUEST
} xlinkActuate;

typedef void (*xlinkNodeDetectFunc) (void *ctx, xmlNode *node);

typedef void
(*xlinkSimpleLinkFunk)	(void *ctx,
			 xmlNode *node,
			 const xlinkHRef href,
			 const xlinkRole role,
			 const xlinkTitle title);

typedef void
(*xlinkExtendedLinkFunk)(void *ctx,
			 xmlNode *node,
			 int nbLocators,
			 const xlinkHRef *hrefs,
			 const xlinkRole *roles,
			 int nbArcs,
			 const xlinkRole *from,
			 const xlinkRole *to,
			 xlinkShow *show,
			 xlinkActuate *actuate,
			 int nbTitles,
			 const xlinkTitle *titles,
			 const xmlChar **langs);

typedef void
(*xlinkExtendedLinkSetFunk)	(void *ctx,
				 xmlNode *node,
				 int nbLocators,
				 const xlinkHRef *hrefs,
				 const xlinkRole *roles,
				 int nbTitles,
				 const xlinkTitle *titles,
				 const xmlChar **langs);

typedef struct _xlinkHandler xlinkHandler;
typedef xlinkHandler *xlinkHandlerPtr;

/**
 * xlinkHandler:
 *
 * Set of XLink detection callbacks (upstream layout: three function
 * pointers).
 */
struct _xlinkHandler {
    xlinkSimpleLinkFunk simple;
    xlinkExtendedLinkFunk extended;
    xlinkExtendedLinkSetFunk set;
};

XMLPUBFUN xlinkNodeDetectFunc
		xlinkGetDefaultDetect	(void);
XMLPUBFUN void
		xlinkSetDefaultDetect	(xlinkNodeDetectFunc func);
XMLPUBFUN xlinkHandler *
		xlinkGetDefaultHandler	(void);
XMLPUBFUN void
		xlinkSetDefaultHandler	(xlinkHandler *handler);
XMLPUBFUN xlinkType
		xlinkIsLink		(xmlDoc *doc,
					 xmlNode *node);

#ifdef __cplusplus
}
#endif

#endif /* __XML_XLINK_H__ */
