/**
 * @file
 *
 * Character validation API (libxml-rs).
 *
 * Headers mirror upstream libxml2 2.15.3 chvalid.h: the character-class
 * tables, the range check and the deprecated xmlIs* family.
 */

#ifndef __XML_CHVALID_H__
#define __XML_CHVALID_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * A range of valid characters.
 */
typedef struct _xmlChSRange xmlChSRange;
typedef xmlChSRange *xmlChSRangePtr;
struct _xmlChSRange {
    unsigned short low;
    unsigned short high;
};

typedef struct _xmlChLRange xmlChLRange;
typedef xmlChLRange *xmlChLRangePtr;
struct _xmlChLRange {
    unsigned int low;
    unsigned int high;
};

typedef struct _xmlChRangeGroup xmlChRangeGroup;
typedef xmlChRangeGroup *xmlChRangeGroupPtr;
struct _xmlChRangeGroup {
    int nbShortRange;
    int nbLongRange;
    const xmlChSRange *shortRange;
    const xmlChLRange *longRange;
};

XMLPUBVAR const xmlChRangeGroup xmlIsBaseCharGroup;
XMLPUBVAR const xmlChRangeGroup xmlIsCharGroup;
XMLPUBVAR const xmlChRangeGroup xmlIsCombiningGroup;
XMLPUBVAR const xmlChRangeGroup xmlIsDigitGroup;
XMLPUBVAR const xmlChRangeGroup xmlIsExtenderGroup;
XMLPUBVAR const xmlChRangeGroup xmlIsIdeographicGroup;
XMLPUBVAR const unsigned char xmlIsPubidChar_tab[256];

/**
 * Range checking routine.
 */
XMLPUBFUN int xmlCharInRange(unsigned int val, const xmlChRangeGroup *group);

XMLPUBFUN int xmlIsBaseChar(unsigned int ch);
XMLPUBFUN int xmlIsBlank(unsigned int ch);
XMLPUBFUN int xmlIsChar(unsigned int ch);
XMLPUBFUN int xmlIsCombining(unsigned int ch);
XMLPUBFUN int xmlIsDigit(unsigned int ch);
XMLPUBFUN int xmlIsExtender(unsigned int ch);
XMLPUBFUN int xmlIsIdeographic(unsigned int ch);
XMLPUBFUN int xmlIsPubidChar(unsigned int ch);

#ifdef __cplusplus
}
#endif

#endif /* __XML_CHVALID_H__ */
