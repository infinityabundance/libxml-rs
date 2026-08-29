/*
 * Summary: internal data structures, constants and functions
 * Description: Internal data structures, constants and functions used
 *              by the XSLT engine.
 *              They are not part of the API or ABI, i.e. they can change
 *              without prior notice, use carefully.
 *
 * Copy: See Copyright for the status of this software.
 *
 * Author: Daniel Veillard
 */

#ifndef __XML_XSLT_INTERNALS_H__
#define __XML_XSLT_INTERNALS_H__

#include <libxml/tree.h>
#include <libxml/hash.h>
#include <libxml/xpath.h>
#include <libxml/xmlerror.h>
#include <libxml/dict.h>
#include <libxml/xmlstring.h>
#include <libxslt/xslt.h>
#include "xsltexports.h"
#include "numbersInternals.h"

#ifdef __cplusplus
extern "C" {
#endif

/* #define XSLT_DEBUG_PROFILE_CACHE */

#define XSLT_IS_TEXT_NODE(n) ((n != NULL) && \
    (((n)->type == XML_TEXT_NODE) || \
     ((n)->type == XML_CDATA_SECTION_NODE)))


#define XSLT_MARK_RES_TREE_FRAG(n) \
    (n)->name = (char *) xmlStrdup(BAD_CAST " fake node libxslt");

#define XSLT_IS_RES_TREE_FRAG(n) \
    ((n != NULL) && ((n)->type == XML_DOCUMENT_NODE) && \
     ((n)->name != NULL) && ((n)->name[0] == ' '))

#define XSLT_REFACTORED_KEYCOMP

#define XSLT_FAST_IF

/* #define XSLT_REFACTORED */
/* ==================================================================== */

#define XSLT_REFACTORED_VARS

#ifdef XSLT_REFACTORED

extern const xmlChar *xsltXSLTAttrMarker;


/* TODO: REMOVE: #define XSLT_REFACTORED_EXCLRESNS */

/* TODO: REMOVE: #define XSLT_REFACTORED_NSALIAS */

/* #define XSLT_REFACTORED_XSLT_NSCOMP */

#ifdef XSLT_REFACTORED_XSLT_NSCOMP

extern const xmlChar *xsltConstNamespaceNameXSLT;

#define IS_XSLT_ELEM_FAST(n) \
    (((n) != NULL) && ((n)->ns != NULL) && \
    ((n)->ns->href == xsltConstNamespaceNameXSLT))

#define IS_XSLT_ATTR_FAST(a) \
    (((a) != NULL) && ((a)->ns != NULL) && \
    ((a)->ns->href == xsltConstNamespaceNameXSLT))

#define XSLT_HAS_INTERNAL_NSMAP(s) \
    (((s) != NULL) && ((s)->principal) && \
     ((s)->principal->principalData) && \
     ((s)->principal->principalData->nsMap))

#define XSLT_GET_INTERNAL_NSMAP(s) ((s)->principal->principalData->nsMap)

#else /* XSLT_REFACTORED_XSLT_NSCOMP */

#define IS_XSLT_ELEM_FAST(n) \
    (((n) != NULL) && ((n)->ns != NULL) && \
     (xmlStrEqual((n)->ns->href, XSLT_NAMESPACE)))

#define IS_XSLT_ATTR_FAST(a) \
    (((a) != NULL) && ((a)->ns != NULL) && \
     (xmlStrEqual((a)->ns->href, XSLT_NAMESPACE)))


#endif /* XSLT_REFACTORED_XSLT_NSCOMP */


/* #define XSLT_REFACTORED_MANDATORY_VERSION */

typedef struct _xsltPointerList xsltPointerList;
typedef xsltPointerList *xsltPointerListPtr;
struct _xsltPointerList {
    void **items;
    int number;
    int size;
};

#endif

/* #define XSLT_REFACTORED_PARSING */

#define XSLT_MAX_SORT 15

#define XSLT_PAT_NO_PRIORITY -12345789

typedef struct _xsltRuntimeExtra xsltRuntimeExtra;
typedef xsltRuntimeExtra *xsltRuntimeExtraPtr;
struct _xsltRuntimeExtra {
    void       *info;		/* pointer to the extra data */
    xmlFreeFunc deallocate;	/* pointer to the deallocation routine */
    union {			/* dual-purpose field */
        void   *ptr;		/* data not needing deallocation */
	int    ival;		/* integer value storage */
    } val;
};

#define XSLT_RUNTIME_EXTRA_LST(ctxt, nr) (ctxt)->extras[(nr)].info
#define XSLT_RUNTIME_EXTRA_FREE(ctxt, nr) (ctxt)->extras[(nr)].deallocate
#define	XSLT_RUNTIME_EXTRA(ctxt, nr, typ) (ctxt)->extras[(nr)].val.typ

typedef struct _xsltTemplate xsltTemplate;
typedef xsltTemplate *xsltTemplatePtr;
struct _xsltTemplate {
    struct _xsltTemplate *next;/* chained list sorted by priority */
    struct _xsltStylesheet *style;/* the containing stylesheet */
    xmlChar *match;	/* the matching string */
    float priority;	/* as given from the stylesheet, not computed */
    const xmlChar *name; /* the local part of the name QName */
    const xmlChar *nameURI; /* the URI part of the name QName */
    const xmlChar *mode;/* the local part of the mode QName */
    const xmlChar *modeURI;/* the URI part of the mode QName */
    xmlNodePtr content;	/* the template replacement value */
    xmlNodePtr elem;	/* the source element */

    /*
    * TODO: @inheritedNsNr and @inheritedNs won't be used in the
    *  refactored code.
    */
    int inheritedNsNr;  /* number of inherited namespaces */
    xmlNsPtr *inheritedNs;/* inherited non-excluded namespaces */

    /* Profiling information */
    int nbCalls;        /* the number of time the template was called */
    unsigned long time; /* the time spent in this template */
    void *params;       /* xsl:param instructions */

    int              templNr;		/* Nb of templates in the stack */
    int              templMax;		/* Size of the templtes stack */
    xsltTemplatePtr *templCalledTab;	/* templates called */
    int             *templCountTab;  /* .. and how often */

    /* Conflict resolution */
    int position;
};

typedef struct _xsltDecimalFormat xsltDecimalFormat;
typedef xsltDecimalFormat *xsltDecimalFormatPtr;
struct _xsltDecimalFormat {
    struct _xsltDecimalFormat *next; /* chained list */
    xmlChar *name;
    /* Used for interpretation of pattern */
    xmlChar *digit;
    xmlChar *patternSeparator;
    /* May appear in result */
    xmlChar *minusSign;
    xmlChar *infinity;
    xmlChar *noNumber; /* Not-a-number */
    /* Used for interpretation of pattern and may appear in result */
    xmlChar *decimalPoint;
    xmlChar *grouping;
    xmlChar *percent;
    xmlChar *permille;
    xmlChar *zeroDigit;
    const xmlChar *nsUri;
};

typedef struct _xsltDocument xsltDocument;
typedef xsltDocument *xsltDocumentPtr;
struct _xsltDocument {
    struct _xsltDocument *next;	/* documents are kept in a chained list */
    int main;			/* is this the main document */
    xmlDocPtr doc;		/* the parsed document */
    void *keys;			/* key tables storage */
    struct _xsltDocument *includes; /* subsidiary includes */
    int preproc;		/* pre-processing already done */
    int nbKeysComputed;
};

typedef struct _xsltKeyDef xsltKeyDef;
typedef xsltKeyDef *xsltKeyDefPtr;
struct _xsltKeyDef {
    struct _xsltKeyDef *next;
    xmlNodePtr inst;
    xmlChar *name;
    xmlChar *nameURI;
    xmlChar *match;
    xmlChar *use;
    xmlXPathCompExprPtr comp;
    xmlXPathCompExprPtr usecomp;
    xmlNsPtr *nsList;           /* the namespaces in scope */
    int nsNr;                   /* the number of namespaces in scope */
};

typedef struct _xsltKeyTable xsltKeyTable;
typedef xsltKeyTable *xsltKeyTablePtr;
struct _xsltKeyTable {
    struct _xsltKeyTable *next;
    xmlChar *name;
    xmlChar *nameURI;
    xmlHashTablePtr keys;
};

/*
 * The in-memory structure corresponding to an XSLT Stylesheet.
 * NOTE: most of the content is simply linked from the doc tree
 *       structure, no specific allocation is made.
 */
typedef struct _xsltStylesheet xsltStylesheet;
typedef xsltStylesheet *xsltStylesheetPtr;

typedef struct _xsltTransformContext xsltTransformContext;
typedef xsltTransformContext *xsltTransformContextPtr;

typedef struct _xsltElemPreComp xsltElemPreComp;
typedef xsltElemPreComp *xsltElemPreCompPtr;

typedef void (*xsltTransformFunction) (xsltTransformContextPtr ctxt,
	                               xmlNodePtr node,
				       xmlNodePtr inst,
			               xsltElemPreCompPtr comp);

typedef void (*xsltSortFunc) (xsltTransformContextPtr ctxt, xmlNodePtr *sorts,
			      int nbsorts);

typedef enum {
    XSLT_FUNC_COPY=1,
    XSLT_FUNC_SORT,
    XSLT_FUNC_TEXT,
    XSLT_FUNC_ELEMENT,
    XSLT_FUNC_ATTRIBUTE,
    XSLT_FUNC_COMMENT,
    XSLT_FUNC_PI,
    XSLT_FUNC_COPYOF,
    XSLT_FUNC_VALUEOF,
    XSLT_FUNC_NUMBER,
    XSLT_FUNC_APPLYIMPORTS,
    XSLT_FUNC_CALLTEMPLATE,
    XSLT_FUNC_APPLYTEMPLATES,
    XSLT_FUNC_CHOOSE,
    XSLT_FUNC_IF,
    XSLT_FUNC_FOREACH,
    XSLT_FUNC_DOCUMENT,
    XSLT_FUNC_WITHPARAM,
    XSLT_FUNC_PARAM,
    XSLT_FUNC_VARIABLE,
    XSLT_FUNC_WHEN,
    XSLT_FUNC_EXTENSION
#ifdef XSLT_REFACTORED
    ,
    XSLT_FUNC_OTHERWISE,
    XSLT_FUNC_FALLBACK,
    XSLT_FUNC_MESSAGE,
    XSLT_FUNC_INCLUDE,
    XSLT_FUNC_ATTRSET,
    XSLT_FUNC_LITERAL_RESULT_ELEMENT,
    XSLT_FUNC_UNKOWN_FORWARDS_COMPAT
#endif
} xsltStyleType;

typedef void (*xsltElemPreCompDeallocator) (xsltElemPreCompPtr comp);

struct _xsltElemPreComp {
    xsltElemPreCompPtr next;		/* next item in the global chained
					   list held by xsltStylesheet. */
    xsltStyleType type;		/* type of the element */
    xsltTransformFunction func;	/* handling function */
    xmlNodePtr inst;			/* the node in the stylesheet's tree
					   corresponding to this item */

    /* end of common part */
    xsltElemPreCompDeallocator free;	/* the deallocator */
};

typedef struct _xsltStylePreComp xsltStylePreComp;
typedef xsltStylePreComp *xsltStylePreCompPtr;

#ifdef XSLT_REFACTORED

/*
* Some pointer-list utility functions.
*/
XSLTPUBFUN xsltPointerListPtr XSLTCALL
		xsltPointerListCreate		(int initialSize);
XSLTPUBFUN void XSLTCALL
		xsltPointerListFree		(xsltPointerListPtr list);
XSLTPUBFUN void XSLTCALL
		xsltPointerListClear		(xsltPointerListPtr list);
XSLTPUBFUN int XSLTCALL
		xsltPointerListAddSize		(xsltPointerListPtr list,
						 void *item,
						 int initialSize);


typedef struct _xsltNsListContainer xsltNsListContainer;
typedef xsltNsListContainer *xsltNsListContainerPtr;
struct _xsltNsListContainer {
    xmlNsPtr *list;
    int totalNumber;
    int xpathNumber;
};

#define XSLT_ITEM_COMPATIBILITY_FIELDS \
    xsltElemPreCompPtr next;\
    xsltStyleType type;\
    xsltTransformFunction func;\
    xmlNodePtr inst;

#define XSLT_ITEM_NAVIGATION_FIELDS
/*
    xsltStylePreCompPtr parent;\
    xsltStylePreCompPtr children;\
    xsltStylePreCompPtr nextItem;
*/

#define XSLT_ITEM_NSINSCOPE_FIELDS xsltNsListContainerPtr inScopeNs;

#define XSLT_ITEM_COMMON_FIELDS \
    XSLT_ITEM_COMPATIBILITY_FIELDS \
    XSLT_ITEM_NAVIGATION_FIELDS \
    XSLT_ITEM_NSINSCOPE_FIELDS

struct _xsltStylePreComp {
    xsltElemPreCompPtr next;    /* next item in the global chained
				   list held by xsltStylesheet */
    xsltStyleType type;         /* type of the item */
    xsltTransformFunction func; /* handling function */
    xmlNodePtr inst;		/* the node in the stylesheet's tree
				   corresponding to this item. */
    /* Currently no navigational fields. */
    xsltNsListContainerPtr inScopeNs;
};

typedef struct _xsltStyleBasicEmptyItem xsltStyleBasicEmptyItem;
typedef xsltStyleBasicEmptyItem *xsltStyleBasicEmptyItemPtr;

struct _xsltStyleBasicEmptyItem {
    XSLT_ITEM_COMMON_FIELDS
};

typedef struct _xsltStyleBasicExpressionItem xsltStyleBasicExpressionItem;
typedef xsltStyleBasicExpressionItem *xsltStyleBasicExpressionItemPtr;

struct _xsltStyleBasicExpressionItem {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *select; /* TODO: Change this to "expression". */
    xmlXPathCompExprPtr comp; /* TODO: Change this to compExpr. */
};


typedef struct _xsltStyleItemElement xsltStyleItemElement;
typedef xsltStyleItemElement *xsltStyleItemElementPtr;

struct _xsltStyleItemElement {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *use;
    int      has_use;
    const xmlChar *name;
    int      has_name;
    const xmlChar *ns;
    const xmlChar *nsPrefix;
    int      has_ns;
};

typedef struct _xsltStyleItemAttribute xsltStyleItemAttribute;
typedef xsltStyleItemAttribute *xsltStyleItemAttributePtr;

struct _xsltStyleItemAttribute {
    XSLT_ITEM_COMMON_FIELDS
    const xmlChar *name;
    int      has_name;
    const xmlChar *ns;
    const xmlChar *nsPrefix;
    int      has_ns;
};

typedef struct _xsltStyleItemText xsltStyleItemText;
typedef xsltStyleItemText *xsltStyleItemTextPtr;

struct _xsltStyleItemText {
    XSLT_ITEM_COMMON_FIELDS
    int      noescape;		/* text */
};

typedef xsltStyleBasicEmptyItem xsltStyleItemComment;
typedef xsltStyleItemComment *xsltStyleItemCommentPtr;

typedef struct _xsltStyleItemPI xsltStyleItemPI;
typedef xsltStyleItemPI *xsltStyleItemPIPtr;

struct _xsltStyleItemPI {
    XSLT_ITEM_COMMON_FIELDS
    const xmlChar *name;
    int      has_name;
};

typedef xsltStyleBasicEmptyItem xsltStyleItemApplyImports;
typedef xsltStyleItemApplyImports *xsltStyleItemApplyImportsPtr;

typedef struct _xsltStyleItemApplyTemplates xsltStyleItemApplyTemplates;
typedef xsltStyleItemApplyTemplates *xsltStyleItemApplyTemplatesPtr;

struct _xsltStyleItemApplyTemplates {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *mode;	/* apply-templates */
    const xmlChar *modeURI;	/* apply-templates */
    const xmlChar *select;	/* sort, copy-of, value-of, apply-templates */
    xmlXPathCompExprPtr comp;	/* a precompiled XPath expression */
    /* TODO: with-params */
};

typedef struct _xsltStyleItemCallTemplate xsltStyleItemCallTemplate;
typedef xsltStyleItemCallTemplate *xsltStyleItemCallTemplatePtr;

struct _xsltStyleItemCallTemplate {
    XSLT_ITEM_COMMON_FIELDS

    xsltTemplatePtr templ;	/* call-template */
    const xmlChar *name;	/* element, attribute, pi */
    int      has_name;		/* element, attribute, pi */
    const xmlChar *ns;		/* element */
    int      has_ns;		/* element */
    /* TODO: with-params */
};

typedef struct _xsltStyleItemCopy xsltStyleItemCopy;
typedef xsltStyleItemCopy *xsltStyleItemCopyPtr;

struct _xsltStyleItemCopy {
   XSLT_ITEM_COMMON_FIELDS
    const xmlChar *use;		/* copy, element */
    int      has_use;		/* copy, element */
};

typedef struct _xsltStyleItemIf xsltStyleItemIf;
typedef xsltStyleItemIf *xsltStyleItemIfPtr;

struct _xsltStyleItemIf {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *test;	/* if */
    xmlXPathCompExprPtr comp;	/* a precompiled XPath expression */
};


typedef xsltStyleBasicExpressionItem xsltStyleItemCopyOf;
typedef xsltStyleItemCopyOf *xsltStyleItemCopyOfPtr;

typedef struct _xsltStyleItemValueOf xsltStyleItemValueOf;
typedef xsltStyleItemValueOf *xsltStyleItemValueOfPtr;

struct _xsltStyleItemValueOf {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *select;
    xmlXPathCompExprPtr comp;	/* a precompiled XPath expression */
    int      noescape;
};

typedef struct _xsltStyleItemNumber xsltStyleItemNumber;
typedef xsltStyleItemNumber *xsltStyleItemNumberPtr;

struct _xsltStyleItemNumber {
    XSLT_ITEM_COMMON_FIELDS
    xsltNumberData numdata;	/* number */
};

typedef xsltStyleBasicEmptyItem xsltStyleItemChoose;
typedef xsltStyleItemChoose *xsltStyleItemChoosePtr;

typedef xsltStyleBasicEmptyItem xsltStyleItemFallback;
typedef xsltStyleItemFallback *xsltStyleItemFallbackPtr;

typedef xsltStyleBasicExpressionItem xsltStyleItemForEach;
typedef xsltStyleItemForEach *xsltStyleItemForEachPtr;

typedef struct _xsltStyleItemMessage xsltStyleItemMessage;
typedef xsltStyleItemMessage *xsltStyleItemMessagePtr;

struct _xsltStyleItemMessage {
    XSLT_ITEM_COMMON_FIELDS
    int terminate;
};

typedef struct _xsltStyleItemDocument xsltStyleItemDocument;
typedef xsltStyleItemDocument *xsltStyleItemDocumentPtr;

struct _xsltStyleItemDocument {
    XSLT_ITEM_COMMON_FIELDS
    int      ver11;		/* assigned: in xsltDocumentComp;
                                  read: nowhere;
                                  TODO: Check if we need. */
    const xmlChar *filename;	/* document URL */
    int has_filename;
};


typedef struct _xsltStyleBasicItemVariable xsltStyleBasicItemVariable;
typedef xsltStyleBasicItemVariable *xsltStyleBasicItemVariablePtr;

struct _xsltStyleBasicItemVariable {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *select;
    xmlXPathCompExprPtr comp;

    const xmlChar *name;
    int      has_name;
    const xmlChar *ns;
    int      has_ns;
};

typedef xsltStyleBasicItemVariable xsltStyleItemVariable;
typedef xsltStyleItemVariable *xsltStyleItemVariablePtr;

typedef struct _xsltStyleItemParam xsltStyleItemParam;
typedef xsltStyleItemParam *xsltStyleItemParamPtr;

struct _xsltStyleItemParam {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *select;
    xmlXPathCompExprPtr comp;

    const xmlChar *name;
    int      has_name;
    const xmlChar *ns;
    int      has_ns;
};

typedef xsltStyleBasicItemVariable xsltStyleItemWithParam;
typedef xsltStyleItemWithParam *xsltStyleItemWithParamPtr;

typedef struct _xsltStyleItemSort xsltStyleItemSort;
typedef xsltStyleItemSort *xsltStyleItemSortPtr;

struct _xsltStyleItemSort {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *stype;       /* sort */
    int      has_stype;		/* sort */
    int      number;		/* sort */
    const xmlChar *order;	/* sort */
    int      has_order;		/* sort */
    int      descending;	/* sort */
    const xmlChar *lang;	/* sort */
    int      has_lang;		/* sort */
    const xmlChar *case_order;	/* sort */
    int      lower_first;	/* sort */

    const xmlChar *use;
    int      has_use;

    const xmlChar *select;	/* sort, copy-of, value-of, apply-templates */

    xmlXPathCompExprPtr comp;	/* a precompiled XPath expression */
};


typedef struct _xsltStyleItemWhen xsltStyleItemWhen;
typedef xsltStyleItemWhen *xsltStyleItemWhenPtr;

struct _xsltStyleItemWhen {
    XSLT_ITEM_COMMON_FIELDS

    const xmlChar *test;
    xmlXPathCompExprPtr comp;
};

typedef struct _xsltStyleItemOtherwise xsltStyleItemOtherwise;
typedef xsltStyleItemOtherwise *xsltStyleItemOtherwisePtr;

struct _xsltStyleItemOtherwise {
    XSLT_ITEM_COMMON_FIELDS
};

typedef struct _xsltStyleItemInclude xsltStyleItemInclude;
typedef xsltStyleItemInclude *xsltStyleItemIncludePtr;

struct _xsltStyleItemInclude {
    XSLT_ITEM_COMMON_FIELDS
    xsltDocumentPtr include;
};


typedef struct _xsltStyleItemUknown xsltStyleItemUknown;
typedef xsltStyleItemUknown *xsltStyleItemUknownPtr;
struct _xsltStyleItemUknown {
    XSLT_ITEM_COMMON_FIELDS
};


/*
 * xsltStyleItemExtElement:
 *
 * Reflects extension elements.
 *
 * NOTE: Due to the fact that the structure xsltElemPreComp is most
 * probably already heavily in use out there by users, so we cannot
 * easily change it, we'll create an intermediate structure which will
 * hold an xsltElemPreCompPtr.
 * BIG NOTE: The only problem I see here is that the user processes the
 *  content of the stylesheet tree, possibly he'll lookup the node->psvi
 *  fields in order to find subsequent extension functions.
 *  In this case, the user's code will break, since the node->psvi
 *  field will hold now the xsltStyleItemExtElementPtr and not
 *  the xsltElemPreCompPtr.
 *  However the place where the structure is anchored in the node-tree,
 *  namely node->psvi, has beed already once been moved from node->_private
 *  to node->psvi, so we have a precedent here, which, I think, should allow
 *  us to change such semantics without headaches.
 */
typedef struct _xsltStyleItemExtElement xsltStyleItemExtElement;
typedef xsltStyleItemExtElement *xsltStyleItemExtElementPtr;
struct _xsltStyleItemExtElement {
    XSLT_ITEM_COMMON_FIELDS
    xsltElemPreCompPtr item;
};


typedef struct _xsltEffectiveNs xsltEffectiveNs;
typedef xsltEffectiveNs *xsltEffectiveNsPtr;
struct _xsltEffectiveNs {
    xsltEffectiveNsPtr nextInStore; /* storage next */
    xsltEffectiveNsPtr next; /* next item in the list */
    const xmlChar *prefix;
    const xmlChar *nsName;
    /*
    * Indicates if eclared on the literal result element; dunno if really
    * needed.
    */
    int holdByElem;
};

/*
 * Info for literal result elements.
 * This will be set on the elem->psvi field and will be
 * shared by literal result elements, which have the same
 * excluded result namespaces; i.e., this *won't* be created uniquely
 * for every literal result element.
 */
typedef struct _xsltStyleItemLRElementInfo xsltStyleItemLRElementInfo;
typedef xsltStyleItemLRElementInfo *xsltStyleItemLRElementInfoPtr;
struct _xsltStyleItemLRElementInfo {
    XSLT_ITEM_COMMON_FIELDS
    /*
    * @effectiveNs is the set of effective ns-nodes
    *  on the literal result element, which will be added to the result
    *  element if not already existing in the result tree.
    *  This means that excluded namespaces (via exclude-result-prefixes,
    *  extension-element-prefixes and the XSLT namespace) not added
    *  to the set.
    *  Namespace-aliasing was applied on the @effectiveNs.
    */
    xsltEffectiveNsPtr effectiveNs;

};

#ifdef XSLT_REFACTORED

typedef struct _xsltNsAlias xsltNsAlias;
typedef xsltNsAlias *xsltNsAliasPtr;
struct _xsltNsAlias {
    xsltNsAliasPtr next; /* next in the list */
    xmlNsPtr literalNs;
    xmlNsPtr targetNs;
    xmlDocPtr docOfTargetNs;
};
#endif

#ifdef XSLT_REFACTORED_XSLT_NSCOMP

typedef struct _xsltNsMap xsltNsMap;
typedef xsltNsMap *xsltNsMapPtr;
struct _xsltNsMap {
    xsltNsMapPtr next; /* next in the list */
    xmlDocPtr doc;
    xmlNodePtr elem; /* the element holding the ns-decl */
    xmlNsPtr ns; /* the xmlNs structure holding the XML namespace name */
    const xmlChar *origNsName; /* the original XML namespace name */
    const xmlChar *newNsName; /* the mapped XML namespace name */
};
#endif


typedef struct _xsltPrincipalStylesheetData xsltPrincipalStylesheetData;
typedef xsltPrincipalStylesheetData *xsltPrincipalStylesheetDataPtr;

typedef struct _xsltNsList xsltNsList;
typedef xsltNsList *xsltNsListPtr;
struct _xsltNsList {
    xsltNsListPtr next; /* next in the list */
    xmlNsPtr ns;
};

/*
* xsltVarInfo:
*
* Used at compilation time for parameters and variables.
*/
typedef struct _xsltVarInfo xsltVarInfo;
typedef xsltVarInfo *xsltVarInfoPtr;
struct _xsltVarInfo {
    xsltVarInfoPtr next; /* next in the list */
    xsltVarInfoPtr prev;
    int depth; /* the depth in the tree */
    const xmlChar *name;
    const xmlChar *nsName;
};

typedef struct _xsltCompilerNodeInfo xsltCompilerNodeInfo;
typedef xsltCompilerNodeInfo *xsltCompilerNodeInfoPtr;
struct _xsltCompilerNodeInfo {
    xsltCompilerNodeInfoPtr next;
    xsltCompilerNodeInfoPtr prev;
    xmlNodePtr node;
    int depth;
    xsltTemplatePtr templ;   /* The owning template */
    int category;	     /* XSLT element, LR-element or
                                extension element */
    xsltStyleType type;
    xsltElemPreCompPtr item; /* The compiled information */
    /* The current in-scope namespaces */
    xsltNsListContainerPtr inScopeNs;
    /* The current excluded result namespaces */
    xsltPointerListPtr exclResultNs;
    /* The current extension instruction namespaces */
    xsltPointerListPtr extElemNs;

    /* The current info for literal result elements. */
    xsltStyleItemLRElementInfoPtr litResElemInfo;
    /*
    * Set to 1 if in-scope namespaces changed,
    *  or excluded result namespaces changed,
    *  or extension element namespaces changed.
    * This will trigger creation of new infos
    *  for literal result elements.
    */
    int nsChanged;
    int preserveWhitespace;
    int stripWhitespace;
    int isRoot; /* whether this is the stylesheet's root node */
    int forwardsCompat; /* whether forwards-compatible mode is enabled */
    /* whether the content of an extension element was processed */
    int extContentHandled;
    /* the type of the current child */
    xsltStyleType curChildType;
};

#define XSLT_CCTXT(style) ((xsltCompilerCtxtPtr) style->compCtxt)

typedef enum {
    XSLT_ERROR_SEVERITY_ERROR = 0,
    XSLT_ERROR_SEVERITY_WARNING
} xsltErrorSeverityType;

typedef struct _xsltCompilerCtxt xsltCompilerCtxt;
typedef xsltCompilerCtxt *xsltCompilerCtxtPtr;
struct _xsltCompilerCtxt {
    void *errorCtxt;            /* user specific error context */
    /*
    * used for error/warning reports; e.g. XSLT_ERROR_SEVERITY_WARNING */
    xsltErrorSeverityType errSeverity;
    int warnings;		/* TODO: number of warnings found at
                                   compilation */
    int errors;			/* TODO: number of errors found at
                                   compilation */
    xmlDictPtr dict;
    xsltStylesheetPtr style;
    int simplified; /* whether this is a simplified stylesheet */
    /* TODO: structured/unstructured error contexts. */
    int depth; /* Current depth of processing */

    xsltCompilerNodeInfoPtr inode;
    xsltCompilerNodeInfoPtr inodeList;
    xsltCompilerNodeInfoPtr inodeLast;
    xsltPointerListPtr tmpList; /* Used for various purposes */
    /*
    * The XSLT version as specified by the stylesheet's root element.
    */
    int isInclude;
    int hasForwardsCompat; /* whether forwards-compatible mode was used
			     in a parsing episode */
    int maxNodeInfos; /* TEMP TODO: just for the interest */
    int maxLREs;  /* TEMP TODO: just for the interest */
    /*
    * In order to keep the old behaviour, applying strict rules of
    * the spec can be turned off. This has effect only on special
    * mechanisms like whitespace-stripping in the stylesheet.
    */
    int strict;
    xsltPrincipalStylesheetDataPtr psData;
    xsltStyleItemUknownPtr unknownItem;
    int hasNsAliases; /* Indicator if there was an xsl:namespace-alias. */
    xsltNsAliasPtr nsAliases;
    xsltVarInfoPtr ivars; /* Storage of local in-scope variables/params. */
    xsltVarInfoPtr ivar; /* topmost local variable/param. */
};

#else /* XSLT_REFACTORED */
/*
* The old structures before refactoring.
*/

struct _xsltStylePreComp {
    xsltElemPreCompPtr next;	/* chained list */
    xsltStyleType type;		/* type of the element */
    xsltTransformFunction func; /* handling function */
    xmlNodePtr inst;		/* the instruction */

    /*
     * Pre computed values.
     */

    const xmlChar *stype;       /* sort */
    int      has_stype;		/* sort */
    int      number;		/* sort */
    const xmlChar *order;	/* sort */
    int      has_order;		/* sort */
    int      descending;	/* sort */
    const xmlChar *lang;	/* sort */
    int      has_lang;		/* sort */
    const xmlChar *case_order;	/* sort */
    int      lower_first;	/* sort */

    const xmlChar *use;		/* copy, element */
    int      has_use;		/* copy, element */

    int      noescape;		/* text */

    const xmlChar *name;	/* element, attribute, pi */
    int      has_name;		/* element, attribute, pi */
    const xmlChar *ns;		/* element */
    int      has_ns;		/* element */

    const xmlChar *mode;	/* apply-templates */
    const xmlChar *modeURI;	/* apply-templates */

    const xmlChar *test;	/* if */

    xsltTemplatePtr templ;	/* call-template */

    const xmlChar *select;	/* sort, copy-of, value-of, apply-templates */

    int      ver11;		/* document */
    const xmlChar *filename;	/* document URL */
    int      has_filename;	/* document */

    xsltNumberData numdata;	/* number */

    xmlXPathCompExprPtr comp;	/* a precompiled XPath expression */
    xmlNsPtr *nsList;		/* the namespaces in scope */
    int nsNr;			/* the number of namespaces in scope */
};

#endif /* XSLT_REFACTORED */


/*
 * The in-memory structure corresponding to an XSLT Variable
 * or Param.
 */
typedef struct _xsltStackElem xsltStackElem;
typedef xsltStackElem *xsltStackElemPtr;
struct _xsltStackElem {
    struct _xsltStackElem *next;/* chained list */
    xsltStylePreCompPtr comp;   /* the compiled form */
    int computed;		/* was the evaluation done */
    const xmlChar *name;	/* the local part of the name QName */
    const xmlChar *nameURI;	/* the URI part of the name QName */
    const xmlChar *select;	/* the eval string */
    xmlNodePtr tree;		/* the sequence constructor if no eval
				    string or the location */
    xmlXPathObjectPtr value;	/* The value if computed */
    xmlDocPtr fragment;		/* The Result Tree Fragments (needed for XSLT 1.0)
				   which are bound to the variable's lifetime. */
    int level;                  /* the depth in the tree;
                                   -1 if persistent (e.g. a given xsl:with-param) */
    xsltTransformContextPtr context; /* The transformation context; needed to cache
                                        the variables */
    int flags;
};

#ifdef XSLT_REFACTORED

struct _xsltPrincipalStylesheetData {
    /*
    * Namespace dictionary for ns-prefixes and ns-names:
    * TODO: Shared between stylesheets, and XPath mechanisms.
    *   Not used yet.
    */
    xmlDictPtr namespaceDict;
    /*
    * Global list of in-scope namespaces.
    */
    xsltPointerListPtr inScopeNamespaces;
    /*
    * Global list of information for [xsl:]excluded-result-prefixes.
    */
    xsltPointerListPtr exclResultNamespaces;
    /*
    * Global list of information for [xsl:]extension-element-prefixes.
    */
    xsltPointerListPtr extElemNamespaces;
    xsltEffectiveNsPtr effectiveNs;
#ifdef XSLT_REFACTORED_XSLT_NSCOMP
    /*
    * Namespace name map to get rid of string comparison of namespace names.
    */
    xsltNsMapPtr nsMap;
#endif
};


#endif
/*
 * Note that we added a @compCtxt field to anchor an stylesheet compilation
 * context, since, due to historical reasons, various compile-time function
 * take only the stylesheet as argument and not a compilation context.
 */
struct _xsltStylesheet {
    /*
     * The stylesheet import relation is kept as a tree.
     */
    struct _xsltStylesheet *parent;
    struct _xsltStylesheet *next;
    struct _xsltStylesheet *imports;

    xsltDocumentPtr docList;		/* the include document list */

    /*
     * General data on the style sheet document.
     */
    xmlDocPtr doc;		/* the parsed XML stylesheet */
    xmlHashTablePtr stripSpaces;/* the hash table of the strip-space and
				   preserve space elements */
    int             stripAll;	/* strip-space * (1) preserve-space * (-1) */
    xmlHashTablePtr cdataSection;/* the hash table of the cdata-section */

    /*
     * Global variable or parameters.
     */
    xsltStackElemPtr variables; /* linked list of param and variables */

    /*
     * Template descriptions.
     */
    xsltTemplatePtr templates;           /* the ordered list of templates */
    xmlHashTablePtr templatesHash;       /* hash table or wherever compiled
                                            templates information is stored */
    struct _xsltCompMatch *rootMatch;    /* template based on / */
    struct _xsltCompMatch *keyMatch;     /* template based on key() */
    struct _xsltCompMatch *elemMatch;    /* template based on * */
    struct _xsltCompMatch *attrMatch;    /* template based on @* */
    struct _xsltCompMatch *parentMatch;  /* template based on .. */
    struct _xsltCompMatch *textMatch;    /* template based on text() */
    struct _xsltCompMatch *piMatch;      /* template based on
                                            processing-instruction() */
    struct _xsltCompMatch *commentMatch; /* template based on comment() */

    /*
     * Namespace aliases.
     * NOTE: Not used in the refactored code.
     */
    xmlHashTablePtr nsAliases;	/* the namespace alias hash tables */

    /*
     * Attribute sets.
     */
    xmlHashTablePtr attributeSets;/* the attribute sets hash tables */

    /*
     * Namespaces.
     * TODO: Eliminate this.
     */
    xmlHashTablePtr nsHash;     /* the set of namespaces in use:
                                   ATTENTION: This is used for
                                   execution of XPath expressions; unfortunately
                                   it restricts the stylesheet to have distinct
                                   prefixes.
				   TODO: We need to get rid of this.
				 */
    void           *nsDefs;     /* ATTENTION TODO: This is currently used to store
				   xsltExtDefPtr (in extensions.c) and
                                   *not* xmlNsPtr.
				 */

    /*
     * Key definitions.
     */
    void *keys;			/* key definitions */

    /*
     * Output related stuff.
     */
    xmlChar *method;		/* the output method */
    xmlChar *methodURI;		/* associated namespace if any */
    xmlChar *version;		/* version string */
    xmlChar *encoding;		/* encoding string */
    int omitXmlDeclaration;     /* omit-xml-declaration = "yes" | "no" */

    /*
     * Number formatting.
     */
    xsltDecimalFormatPtr decimalFormat;
    int standalone;             /* standalone = "yes" | "no" */
    xmlChar *doctypePublic;     /* doctype-public string */
    xmlChar *doctypeSystem;     /* doctype-system string */
    int indent;			/* should output being indented */
    xmlChar *mediaType;		/* media-type string */

    /*
     * Precomputed blocks.
     */
    xsltElemPreCompPtr preComps;/* list of precomputed blocks */
    int warnings;		/* number of warnings found at compilation */
    int errors;			/* number of errors found at compilation */

    xmlChar  *exclPrefix;	/* last excluded prefixes */
    xmlChar **exclPrefixTab;	/* array of excluded prefixes */
    int       exclPrefixNr;	/* number of excluded prefixes in scope */
    int       exclPrefixMax;	/* size of the array */

    void     *_private;		/* user defined data */

    /*
     * Extensions.
     */
    xmlHashTablePtr extInfos;	/* the extension data */
    int		    extrasNr;	/* the number of extras required */

    /*
     * For keeping track of nested includes
     */
    xsltDocumentPtr includes;	/* points to last nested include */

    /*
     * dictionary: shared between stylesheet, context and documents.
     */
    xmlDictPtr dict;
    /*
     * precompiled attribute value templates.
     */
    void *attVTs;
    /*
     * if namespace-alias has an alias for the default stylesheet prefix
     * NOTE: Not used in the refactored code.
     */
    const xmlChar *defaultAlias;
    /*
     * bypass pre-processing (already done) (used in imports)
     */
    int nopreproc;
    /*
     * all document text strings were internalized
     */
    int internalized;
    /*
     * Literal Result Element as Stylesheet c.f. section 2.3
     */
    int literal_result;
    /*
    * The principal stylesheet
    */
    xsltStylesheetPtr principal;
#ifdef XSLT_REFACTORED
    /*
    * Compilation context used during compile-time.
    */
    xsltCompilerCtxtPtr compCtxt; /* TODO: Change this to (void *). */

    xsltPrincipalStylesheetDataPtr principalData;
#endif
    /*
     * Forwards-compatible processing
     */
    int forwards_compatible;

    xmlHashTablePtr namedTemplates; /* hash table of named templates */

    xmlXPathContextPtr xpathCtxt;

    unsigned long opLimit;
    unsigned long opCount;
};

typedef struct _xsltTransformCache xsltTransformCache;
typedef xsltTransformCache *xsltTransformCachePtr;
struct _xsltTransformCache {
    xmlDocPtr RVT;
    int nbRVT;
    xsltStackElemPtr stackItems;
    int nbStackItems;
#ifdef XSLT_DEBUG_PROFILE_CACHE
    int dbgCachedRVTs;
    int dbgReusedRVTs;
    int dbgCachedVars;
    int dbgReusedVars;
#endif
};

/*
 * The in-memory structure corresponding to an XSLT Transformation.
 */
typedef enum {
    XSLT_OUTPUT_XML = 0,
    XSLT_OUTPUT_HTML,
    XSLT_OUTPUT_TEXT
} xsltOutputType;

typedef void *
(*xsltNewLocaleFunc)(const xmlChar *lang, int lowerFirst);
typedef void
(*xsltFreeLocaleFunc)(void *locale);
typedef xmlChar *
(*xsltGenSortKeyFunc)(void *locale, const xmlChar *lang);

typedef enum {
    XSLT_STATE_OK = 0,
    XSLT_STATE_ERROR,
    XSLT_STATE_STOPPED
} xsltTransformState;

struct _xsltTransformContext {
    xsltStylesheetPtr style;		/* the stylesheet used */
    xsltOutputType type;		/* the type of output */

    xsltTemplatePtr  templ;		/* the current template */
    int              templNr;		/* Nb of templates in the stack */
    int              templMax;		/* Size of the templtes stack */
    xsltTemplatePtr *templTab;		/* the template stack */

    xsltStackElemPtr  vars;		/* the current variable list */
    int               varsNr;		/* Nb of variable list in the stack */
    int               varsMax;		/* Size of the variable list stack */
    xsltStackElemPtr *varsTab;		/* the variable list stack */
    int               varsBase;		/* the var base for current templ */

    /*
     * Extensions
     */
    xmlHashTablePtr   extFunctions;	/* the extension functions */
    xmlHashTablePtr   extElements;	/* the extension elements */
    xmlHashTablePtr   extInfos;		/* the extension data */

    const xmlChar *mode;		/* the current mode */
    const xmlChar *modeURI;		/* the current mode URI */

    xsltDocumentPtr docList;		/* the document list */

    xsltDocumentPtr document;		/* the current source document; can be NULL if an RTF */
    xmlNodePtr node;			/* the current node being processed */
    xmlNodeSetPtr nodeList;		/* the current node list */
    /* xmlNodePtr current;			the node */

    xmlDocPtr output;			/* the resulting document */
    xmlNodePtr insert;			/* the insertion node */

    xmlXPathContextPtr xpathCtxt;	/* the XPath context */
    xsltTransformState state;		/* the current state */

    /*
     * Global variables
     */
    xmlHashTablePtr   globalVars;	/* the global variables and params */

    xmlNodePtr inst;			/* the instruction in the stylesheet */

    int xinclude;			/* should XInclude be processed */

    const char *      outputFile;	/* the output URI if known */

    int profile;                        /* is this run profiled */
    long             prof;		/* the current profiled value */
    int              profNr;		/* Nb of templates in the stack */
    int              profMax;		/* Size of the templtaes stack */
    long            *profTab;		/* the profile template stack */

    void            *_private;		/* user defined data */

    int              extrasNr;		/* the number of extras used */
    int              extrasMax;		/* the number of extras allocated */
    xsltRuntimeExtraPtr extras;		/* extra per runtime information */

    xsltDocumentPtr  styleList;		/* the stylesheet docs list */
    void                 * sec;		/* the security preferences if any */

    xmlGenericErrorFunc  error;		/* a specific error handler */
    void              * errctx;		/* context for the error handler */

    xsltSortFunc      sortfunc;		/* a ctxt specific sort routine */

    /*
     * handling of temporary Result Value Tree
     * (XSLT 1.0 term: "Result Tree Fragment")
     */
    xmlDocPtr       tmpRVT;		/* list of RVT without persistance */
    xmlDocPtr       persistRVT;		/* list of persistant RVTs */
    int             ctxtflags;          /* context processing flags */

    /*
     * Speed optimization when coalescing text nodes
     */
    const xmlChar  *lasttext;		/* last text node content */
    int             lasttsize;		/* last text node size */
    int             lasttuse;		/* last text node use */
    /*
     * Per Context Debugging
     */
    int debugStatus;			/* the context level debug status */
    unsigned long* traceCode;		/* pointer to the variable holding the mask */

    int parserOptions;			/* parser options xmlParserOption */

    /*
     * dictionary: shared between stylesheet, context and documents.
     */
    xmlDictPtr dict;
    xmlDocPtr		tmpDoc; /* Obsolete; not used in the library. */
    /*
     * all document text strings are internalized
     */
    int internalized;
    int nbKeys;
    int hasTemplKeyPatterns;
    xsltTemplatePtr currentTemplateRule; /* the Current Template Rule */
    xmlNodePtr initialContextNode;
    xmlDocPtr initialContextDoc;
    xsltTransformCachePtr cache;
    void *contextVariable; /* the current variable item */
    xmlDocPtr localRVT; /* list of local tree fragments; will be freed when
			   the instruction which created the fragment
                           exits */
    xmlDocPtr localRVTBase; /* Obsolete */
    int keyInitLevel;   /* Needed to catch recursive keys issues */
    int depth;          /* Needed to catch recursions */
    int maxTemplateDepth;
    int maxTemplateVars;
    unsigned long opLimit;
    unsigned long opCount;
    int sourceDocDirty;
    unsigned long currentId; /* For generate-id() */

    xsltNewLocaleFunc newLocale;
    xsltFreeLocaleFunc freeLocale;
    xsltGenSortKeyFunc genSortKey;
};

#define CHECK_STOPPED if (ctxt->state == XSLT_STATE_STOPPED) return;

#define CHECK_STOPPEDE if (ctxt->state == XSLT_STATE_STOPPED) goto error;

#define CHECK_STOPPED0 if (ctxt->state == XSLT_STATE_STOPPED) return(0);

/*
 * The macro XML_CAST_FPTR is a hack to avoid a gcc warning about
 * possible incompatibilities between function pointers and object
 * pointers.  It is defined in libxml/hash.h within recent versions
 * of libxml2, but is put here for compatibility.
 */
#ifndef XML_CAST_FPTR

#define XML_CAST_FPTR(fptr) fptr
#endif
/*
 * Functions associated to the internal types
xsltDecimalFormatPtr	xsltDecimalFormatGetByName(xsltStylesheetPtr sheet,
						   xmlChar *name);
 */
XSLTPUBFUN xsltStylesheetPtr XSLTCALL
			xsltNewStylesheet	(void);
XSLTPUBFUN xsltStylesheetPtr XSLTCALL
			xsltParseStylesheetFile	(const xmlChar* filename);
XSLTPUBFUN void XSLTCALL
			xsltFreeStylesheet	(xsltStylesheetPtr style);
XSLTPUBFUN int XSLTCALL
			xsltIsBlank		(xmlChar *str);
XSLTPUBFUN void XSLTCALL
			xsltFreeStackElemList	(xsltStackElemPtr elem);
XSLTPUBFUN xsltDecimalFormatPtr XSLTCALL
			xsltDecimalFormatGetByName(xsltStylesheetPtr style,
						 xmlChar *name);
XSLTPUBFUN xsltDecimalFormatPtr XSLTCALL
			xsltDecimalFormatGetByQName(xsltStylesheetPtr style,
						 const xmlChar *nsUri,
                                                 const xmlChar *name);

XSLTPUBFUN xsltStylesheetPtr XSLTCALL
			xsltParseStylesheetProcess(xsltStylesheetPtr ret,
						 xmlDocPtr doc);
XSLTPUBFUN void XSLTCALL
			xsltParseStylesheetOutput(xsltStylesheetPtr style,
						 xmlNodePtr cur);
XSLTPUBFUN xsltStylesheetPtr XSLTCALL
			xsltParseStylesheetDoc	(xmlDocPtr doc);
XSLTPUBFUN xsltStylesheetPtr XSLTCALL
			xsltParseStylesheetImportedDoc(xmlDocPtr doc,
						xsltStylesheetPtr style);
XSLTPUBFUN int XSLTCALL
			xsltParseStylesheetUser(xsltStylesheetPtr style,
						xmlDocPtr doc);
XSLTPUBFUN xsltStylesheetPtr XSLTCALL
			xsltLoadStylesheetPI	(xmlDocPtr doc);
XSLTPUBFUN void XSLTCALL
			xsltNumberFormat	(xsltTransformContextPtr ctxt,
						 xsltNumberDataPtr data,
						 xmlNodePtr node);
XSLTPUBFUN xmlXPathError XSLTCALL
			xsltFormatNumberConversion(xsltDecimalFormatPtr self,
						 xmlChar *format,
						 double number,
						 xmlChar **result);

XSLTPUBFUN void XSLTCALL
			xsltParseTemplateContent(xsltStylesheetPtr style,
						 xmlNodePtr templ);
XSLTPUBFUN int XSLTCALL
			xsltAllocateExtra	(xsltStylesheetPtr style);
XSLTPUBFUN int XSLTCALL
			xsltAllocateExtraCtxt	(xsltTransformContextPtr ctxt);
/*
 * Extra functions for Result Value Trees
 */
XSLTPUBFUN xmlDocPtr XSLTCALL
			xsltCreateRVT		(xsltTransformContextPtr ctxt);
XSLTPUBFUN int XSLTCALL
			xsltRegisterTmpRVT	(xsltTransformContextPtr ctxt,
						 xmlDocPtr RVT);
XSLTPUBFUN int XSLTCALL
			xsltRegisterLocalRVT	(xsltTransformContextPtr ctxt,
						 xmlDocPtr RVT);
XSLTPUBFUN int XSLTCALL
			xsltRegisterPersistRVT	(xsltTransformContextPtr ctxt,
						 xmlDocPtr RVT);
XSLTPUBFUN int XSLTCALL
			xsltExtensionInstructionResultRegister(
						 xsltTransformContextPtr ctxt,
						 xmlXPathObjectPtr obj);
XSLTPUBFUN int XSLTCALL
			xsltExtensionInstructionResultFinalize(
						 xsltTransformContextPtr ctxt);
XSLTPUBFUN int XSLTCALL
			xsltFlagRVTs(
						 xsltTransformContextPtr ctxt,
						 xmlXPathObjectPtr obj,
						 int val);
XSLTPUBFUN void XSLTCALL
			xsltFreeRVTs		(xsltTransformContextPtr ctxt);
XSLTPUBFUN void XSLTCALL
			xsltReleaseRVT		(xsltTransformContextPtr ctxt,
						 xmlDocPtr RVT);
/*
 * Extra functions for Attribute Value Templates
 */
XSLTPUBFUN void XSLTCALL
			xsltCompileAttr		(xsltStylesheetPtr style,
						 xmlAttrPtr attr);
XSLTPUBFUN xmlChar * XSLTCALL
			xsltEvalAVT		(xsltTransformContextPtr ctxt,
						 void *avt,
						 xmlNodePtr node);
XSLTPUBFUN void XSLTCALL
			xsltFreeAVTList		(void *avt);

/*
 * Extra function for successful xsltCleanupGlobals / xsltInit sequence.
 */

XSLTPUBFUN void XSLTCALL
			xsltUninit		(void);


#ifdef XSLT_REFACTORED
XSLTPUBFUN void XSLTCALL
			xsltParseSequenceConstructor(
						 xsltCompilerCtxtPtr cctxt,
						 xmlNodePtr start);
XSLTPUBFUN int XSLTCALL
			xsltParseAnyXSLTElem	(xsltCompilerCtxtPtr cctxt,
						 xmlNodePtr elem);
#ifdef XSLT_REFACTORED_XSLT_NSCOMP
XSLTPUBFUN int XSLTCALL
			xsltRestoreDocumentNamespaces(
						 xsltNsMapPtr ns,
						 xmlDocPtr doc);
#endif
#endif /* XSLT_REFACTORED */

XSLTPUBFUN int XSLTCALL
			xsltInitCtxtKey		(xsltTransformContextPtr ctxt,
						 xsltDocumentPtr doc,
						 xsltKeyDefPtr keyd);
XSLTPUBFUN int XSLTCALL
			xsltInitAllDocKeys	(xsltTransformContextPtr ctxt);
#ifdef __cplusplus
}
#endif

#endif /* __XML_XSLT_H__ */

