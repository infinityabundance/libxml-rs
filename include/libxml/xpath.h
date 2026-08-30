/**
 * @file
 *
 * XPath API for libxml-rs
 */

#ifndef __XML_XPATH_H__
#define __XML_XPATH_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxml/hash.h>
#include <libxml/dict.h>

/* Forward declarations */
struct _xmlXPathParserContext;

typedef struct _xmlXPathParserContext xmlXPathParserContext;
typedef xmlXPathParserContext *xmlXPathParserContextPtr;
typedef struct _xmlXPathCompExpr xmlXPathCompExpr;
typedef xmlXPathCompExpr *xmlXPathCompExprPtr;

#ifdef __cplusplus
extern "C" {
#endif

/* XPath object types */
typedef enum {
    XPATH_UNDEFINED = 0,
    XPATH_NODESET = 1,
    XPATH_BOOLEAN = 2,
    XPATH_NUMBER = 3,
    XPATH_STRING = 4,
    XPATH_POINT = 5,
    XPATH_RANGE = 6,
    XPATH_LOCATIONSET = 7,
    XPATH_USERS = 8,
    XPATH_XSLT_TREE = 9
} xmlXPathObjectType;

/* Node set */
typedef struct _xmlNodeSet xmlNodeSet;
typedef xmlNodeSet *xmlNodeSetPtr;
struct _xmlNodeSet {
    int nodeNr;
    int nodeMax;
    xmlNodePtr *nodeTab;
};

/* XPath object */
typedef struct _xmlXPathObject xmlXPathObject;
typedef xmlXPathObject *xmlXPathObjectPtr;
struct _xmlXPathObject {
    int type;
    xmlNodeSetPtr nodesetval;
    int boolval;
    double floatval;
    xmlChar *stringval;
    void *user;
    int index;
    void *user2;
    int index2;
};

/* Callback types (must be defined before XPath context) */
typedef void (*xmlXPathFunction)(xmlXPathParserContextPtr ctxt, int nargs);
typedef xmlXPathObjectPtr (*xmlXPathVariableLookupFunc)(void *ctxt,
                                                         const xmlChar *name,
                                                         const xmlChar *ns_uri);
typedef xmlXPathFunction (*xmlXPathFuncLookupFunc)(void *ctxt,
                                                    const xmlChar *name,
                                                    const xmlChar *ns_uri);

/* XPath context */
typedef struct _xmlXPathContext xmlXPathContext;
typedef xmlXPathContext *xmlXPathContextPtr;
struct _xmlXPathContext {
    xmlDocPtr doc;
    xmlNodePtr node;
    int nb_variables_unused;
    int max_variables_unused;
    void *varHash;
    int nb_types;
    int max_types;
    void *types;
    int nb_funcs_unused;
    int max_funcs_unused;
    void *funcHash;
    int nb_axis;
    int max_axis;
    void **axis;
    xmlNsPtr *namespaces;
    int nsNr;
    void *user;
    int contextSize;
    int proximityPosition;
    int xptr;
    xmlNodePtr here;
    xmlNodePtr origin;
    void *nsHash;
    xmlXPathVariableLookupFunc varLookupFunc;
    void *varLookupData;
    void *extra;
    xmlXPathFunction function;
    const xmlChar *functionURI;
    xmlXPathFuncLookupFunc funcLookupFunc;
    void *funcLookupData;
    xmlNsPtr *tmpNsList;
    int tmpNsNr;
    void *userData;
    xmlGenericErrorFunc error;
    xmlError lastError;
    xmlNodePtr debugNode;
    xmlDictPtr dict;
    int flags;
    void *cache;
    unsigned long opLimit;
    unsigned long opCount;
    int depth;
};

/* XPath API */
XMLPUBFUN xmlXPathContextPtr xmlXPathNewContext(xmlDocPtr doc);
XMLPUBFUN void xmlXPathFreeContext(xmlXPathContextPtr ctxt);
XMLPUBFUN void xmlXPathSetErrorHandler(xmlXPathContextPtr ctxt,
                                      xmlStructuredErrorFunc handler,
                                      void *context);
XMLPUBFUN int xmlXPathContextSetCache(xmlXPathContextPtr ctxt,
                                     int active, int value, int options);
XMLPUBFUN xmlXPathObjectPtr xmlXPathEvalExpression(const xmlChar *str,
                                                    xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr xmlXPathEval(const xmlChar *str,
                                          xmlXPathContextPtr ctxt);
XMLPUBFUN int xmlXPathSetContextNode(xmlNodePtr node, xmlXPathContextPtr ctx);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNodeEval(xmlNodePtr node,
                                            const xmlChar *str,
                                            xmlXPathContextPtr ctx);
XMLPUBFUN int xmlXPathEvalPredicate(xmlXPathContextPtr ctxt,
                                    xmlXPathObjectPtr res);
XMLPUBFUN void xmlXPathFreeObject(xmlXPathObjectPtr obj);
XMLPUBFUN void *xmlXPathCompile(const xmlChar *str);
XMLPUBFUN void xmlXPathFreeCompExpr(void *comp);
XMLPUBFUN int xmlXPathRegisterNs(xmlXPathContextPtr ctxt,
                                  const xmlChar *prefix, const xmlChar *ns_uri);
XMLPUBFUN int xmlXPathRegisterFunc(xmlXPathContextPtr ctxt,
                                    const xmlChar *name, xmlXPathFunction f);
XMLPUBFUN int xmlXPathRegisterFuncNS(xmlXPathContextPtr ctxt,
                                      const xmlChar *name, const xmlChar *ns_uri,
                                      xmlXPathFunction f);
XMLPUBFUN int xmlXPathRegisterVariable(xmlXPathContextPtr ctxt,
                                        const xmlChar *name,
                                        xmlXPathObjectPtr value);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewNodeSet(xmlNodePtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewCString(const xmlChar *val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewFloat(double val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewBoolean(int val);

XMLPUBFUN xmlXPathObjectPtr xmlXPathNewString(const xmlChar *val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewValueTree(xmlNodePtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathNewNodeSetList(xmlNodeSetPtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathWrapString(xmlChar *val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathWrapCString(char *val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathWrapNodeSet(xmlNodeSetPtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathWrapExternal(void *val);
XMLPUBFUN void xmlXPathFreeNodeSetList(xmlXPathObjectPtr obj);

XMLPUBFUN xmlXPathObjectPtr xmlXPathObjectCopy(xmlXPathObjectPtr val);
XMLPUBFUN int xmlXPathCmpNodes(xmlNodePtr node1, xmlNodePtr node2);
XMLPUBFUN xmlNodeSetPtr xmlXPathNodeSetCreate(xmlNodePtr val);
XMLPUBFUN void xmlXPathFreeNodeSet(xmlNodeSetPtr obj);

XMLPUBFUN xmlXPathObjectPtr xmlXPathConvertBoolean(xmlXPathObjectPtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathConvertNumber(xmlXPathObjectPtr val);
XMLPUBFUN xmlXPathObjectPtr xmlXPathConvertString(xmlXPathObjectPtr val);

XMLPUBFUN int xmlXPathCastToBoolean(xmlXPathObjectPtr val);
XMLPUBFUN double xmlXPathCastToNumber(xmlXPathObjectPtr val);
XMLPUBFUN xmlChar *xmlXPathCastToString(xmlXPathObjectPtr val);
XMLPUBFUN int xmlXPathCastBooleanToNumber(int val);
XMLPUBFUN xmlChar *xmlXPathCastBooleanToString(int val);
XMLPUBFUN int xmlXPathCastNodeSetToBoolean(xmlNodeSetPtr ns);
XMLPUBFUN double xmlXPathCastNodeSetToNumber(xmlNodeSetPtr ns);
XMLPUBFUN xmlChar *xmlXPathCastNodeSetToString(xmlNodeSetPtr ns);
XMLPUBFUN double xmlXPathCastNodeToNumber(xmlNodePtr node);
XMLPUBFUN xmlChar *xmlXPathCastNodeToString(xmlNodePtr node);
XMLPUBFUN int xmlXPathCastNumberToBoolean(double val);
XMLPUBFUN xmlChar *xmlXPathCastNumberToString(double val);
XMLPUBFUN int xmlXPathCastStringToBoolean(const xmlChar *val);
XMLPUBFUN double xmlXPathCastStringToNumber(const xmlChar *val);

XMLPUBFUN int xmlXPathIsNaN(double val);
XMLPUBFUN int xmlXPathIsInf(double val);
XMLPUBFUN double xmlXPathStringEvalNumber(const xmlChar *str);
XMLPUBFUN int xmlXPathIsNodeType(const xmlChar *name);
XMLPUBFUN void xmlXPathInit(void);
XMLPUBFUN void xmlXPathErr(xmlXPathParserContextPtr ctxt, int error);
XMLPUBFUN void xmlXPatherror(xmlXPathParserContextPtr ctxt,
                             const char *file, int line, int no);

XMLPUBFUN int xmlXPathNodeSetContains(xmlNodeSetPtr cur, xmlNodePtr val);
XMLPUBFUN int xmlXPathNodeSetAdd(xmlNodeSetPtr cur, xmlNodePtr val);
XMLPUBFUN int xmlXPathNodeSetAddUnique(xmlNodeSetPtr cur, xmlNodePtr val);
XMLPUBFUN int xmlXPathNodeSetAddNs(xmlNodeSetPtr cur, xmlNodePtr node,
                                  xmlNsPtr ns);
/* xmlXPathNodeSetDel/xmlXPathNodeSetRemove are declared (as void) in
 * xpathInternals.h — the upstream location; removing the int prototypes here
 * (they are xpathInternals, not public xpath.h, declarations). */
XMLPUBFUN void xmlXPathNodeSetSort(xmlNodeSetPtr set);
XMLPUBFUN xmlNodeSetPtr xmlXPathNodeSetMerge(xmlNodeSetPtr val1,
                                            xmlNodeSetPtr val2);
XMLPUBFUN xmlNodeSetPtr xmlXPathDifference(xmlNodeSetPtr nodes1,
                                          xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathIntersection(xmlNodeSetPtr nodes1,
                                            xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathDistinct(xmlNodeSetPtr nodes);
XMLPUBFUN xmlNodeSetPtr xmlXPathDistinctSorted(xmlNodeSetPtr nodes);
XMLPUBFUN int xmlXPathHasSameNodes(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathLeading(xmlNodeSetPtr nodes1,
                                       xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathLeadingSorted(xmlNodeSetPtr nodes1,
                                             xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathTrailing(xmlNodeSetPtr nodes1,
                                        xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathTrailingSorted(xmlNodeSetPtr nodes1,
                                              xmlNodeSetPtr nodes2);
XMLPUBFUN xmlNodeSetPtr xmlXPathNodeLeading(xmlNodeSetPtr nodes,
                                           xmlNodePtr node);
XMLPUBFUN xmlNodeSetPtr xmlXPathNodeLeadingSorted(xmlNodeSetPtr nodes,
                                                 xmlNodePtr node);
XMLPUBFUN xmlNodeSetPtr xmlXPathNodeTrailing(xmlNodeSetPtr nodes,
                                            xmlNodePtr node);
XMLPUBFUN xmlNodeSetPtr xmlXPathNodeTrailingSorted(xmlNodeSetPtr nodes,
                                                  xmlNodePtr node);
XMLPUBFUN void xmlXPathNodeSetFreeNs(xmlNsPtr ns);
XMLPUBFUN long xmlXPathOrderDocElems(xmlDocPtr doc);

/* xmlXPathValuePush / xmlXPathValuePop (and the Pop / Return helper
 * families) are xpathInternals.h declarations upstream — the canonical
 * (and corrected) prototypes live there. */
XMLPUBFUN int xmlXPathPopBoolean(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void *xmlXPathPopExternal(xmlXPathParserContextPtr ctxt);
XMLPUBFUN xmlNodeSetPtr xmlXPathPopNodeSet(xmlXPathParserContextPtr ctxt);
XMLPUBFUN double xmlXPathPopNumber(xmlXPathParserContextPtr ctxt);
XMLPUBFUN xmlChar *xmlXPathPopString(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathAddValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathSubValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathMultValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathDivValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathModValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathValueFlipSign(xmlXPathParserContextPtr ctxt);
XMLPUBFUN int xmlXPathEqualValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN int xmlXPathNotEqualValues(xmlXPathParserContextPtr ctxt);
XMLPUBFUN int xmlXPathCompareValues(xmlXPathParserContextPtr ctxt,
                                   int inf, int strict);

XMLPUBFUN xmlXPathParserContextPtr
        xmlXPathNewParserContext(const xmlChar *str, xmlXPathContextPtr ctxt);
XMLPUBFUN void xmlXPathFreeParserContext(xmlXPathParserContextPtr ctxt);
XMLPUBFUN xmlChar *xmlXPathParseName(xmlXPathParserContextPtr ctxt);
XMLPUBFUN xmlChar *xmlXPathParseNCName(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathRoot(xmlXPathParserContextPtr ctxt);
XMLPUBFUN void xmlXPathEvalExpr(xmlXPathParserContextPtr ctxt);
XMLPUBFUN int xmlXPathEvaluatePredicateResult(xmlXPathParserContextPtr ctxt,
                                              xmlXPathObjectPtr res);

XMLPUBFUN xmlNodePtr xmlXPathNextAncestor(xmlXPathParserContextPtr ctxt,
                                         xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextAncestorOrSelf(xmlXPathParserContextPtr ctxt,
                                               xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextAttribute(xmlXPathParserContextPtr ctxt,
                                          xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextChild(xmlXPathParserContextPtr ctxt,
                                      xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextDescendant(xmlXPathParserContextPtr ctxt,
                                           xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextDescendantOrSelf(xmlXPathParserContextPtr ctxt,
                                                 xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextFollowing(xmlXPathParserContextPtr ctxt,
                                          xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextFollowingSibling(xmlXPathParserContextPtr ctxt,
                                                 xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextNamespace(xmlXPathParserContextPtr ctxt,
                                          xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextParent(xmlXPathParserContextPtr ctxt,
                                       xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextPreceding(xmlXPathParserContextPtr ctxt,
                                          xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextPrecedingSibling(xmlXPathParserContextPtr ctxt,
                                                 xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlXPathNextSelf(xmlXPathParserContextPtr ctxt,
                                     xmlNodePtr cur);

XMLPUBFUN void xmlXPathBooleanFunction(xmlXPathParserContextPtr ctxt,
                                       int nargs);
XMLPUBFUN void xmlXPathNotFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathTrueFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathFalseFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathLangFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathNumberFunction(xmlXPathParserContextPtr ctxt,
                                     int nargs);
XMLPUBFUN void xmlXPathSumFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathFloorFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathCeilingFunction(xmlXPathParserContextPtr ctxt,
                                      int nargs);
XMLPUBFUN void xmlXPathRoundFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathLastFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathPositionFunction(xmlXPathParserContextPtr ctxt,
                                       int nargs);
XMLPUBFUN void xmlXPathCountFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathIdFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathLocalNameFunction(xmlXPathParserContextPtr ctxt,
                                        int nargs);
XMLPUBFUN void xmlXPathNamespaceURIFunction(xmlXPathParserContextPtr ctxt,
                                           int nargs);
XMLPUBFUN void xmlXPathStringFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathStringLengthFunction(xmlXPathParserContextPtr ctxt,
                                           int nargs);
XMLPUBFUN void xmlXPathConcatFunction(xmlXPathParserContextPtr ctxt, int nargs);
XMLPUBFUN void xmlXPathContainsFunction(xmlXPathParserContextPtr ctxt,
                                       int nargs);
XMLPUBFUN void xmlXPathStartsWithFunction(xmlXPathParserContextPtr ctxt,
                                         int nargs);
XMLPUBFUN void xmlXPathSubstringFunction(xmlXPathParserContextPtr ctxt,
                                        int nargs);
XMLPUBFUN void xmlXPathSubstringBeforeFunction(xmlXPathParserContextPtr ctxt,
                                              int nargs);
XMLPUBFUN void xmlXPathSubstringAfterFunction(xmlXPathParserContextPtr ctxt,
                                             int nargs);
XMLPUBFUN void xmlXPathNormalizeFunction(xmlXPathParserContextPtr ctxt,
                                        int nargs);
XMLPUBFUN void xmlXPathTranslateFunction(xmlXPathParserContextPtr ctxt,
                                        int nargs);

XMLPUBFUN void xmlXPathRegisterAllFunctions(xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathFunction xmlXPathFunctionLookup(xmlXPathContextPtr ctxt,
                                                 const xmlChar *name);
XMLPUBFUN xmlXPathFunction xmlXPathFunctionLookupNS(xmlXPathContextPtr ctxt,
                                                   const xmlChar *name,
                                                   const xmlChar *ns_uri);
XMLPUBFUN xmlXPathObjectPtr xmlXPathVariableLookup(xmlXPathContextPtr ctxt,
                                                  const xmlChar *name);
XMLPUBFUN xmlXPathObjectPtr xmlXPathVariableLookupNS(xmlXPathContextPtr ctxt,
                                                    const xmlChar *name,
                                                    const xmlChar *ns_uri);
XMLPUBFUN const xmlChar *xmlXPathNsLookup(xmlXPathContextPtr ctxt,
                                         const xmlChar *prefix);
XMLPUBFUN void xmlXPathRegisterFuncLookup(xmlXPathContextPtr ctxt,
                                         xmlXPathFuncLookupFunc f,
                                         void *funcCtxt);
XMLPUBFUN void xmlXPathRegisterVariableLookup(xmlXPathContextPtr ctxt,
                                             xmlXPathVariableLookupFunc f,
                                             void *data);
XMLPUBFUN int xmlXPathRegisterVariableNS(xmlXPathContextPtr ctxt,
                                        const xmlChar *name,
                                        const xmlChar *ns_uri,
                                        xmlXPathObjectPtr value);
XMLPUBFUN void xmlXPathRegisteredFuncsCleanup(xmlXPathContextPtr ctxt);
XMLPUBFUN void xmlXPathRegisteredNsCleanup(xmlXPathContextPtr ctxt);
XMLPUBFUN void xmlXPathRegisteredVariablesCleanup(xmlXPathContextPtr ctxt);

XMLPUBFUN xmlXPathCompExprPtr xmlXPathCtxtCompile(xmlXPathContextPtr ctxt,
                                                 const xmlChar *str);
XMLPUBFUN xmlXPathObjectPtr xmlXPathCompiledEval(xmlXPathCompExprPtr comp,
                                                xmlXPathContextPtr ctx);
XMLPUBFUN int xmlXPathCompiledEvalToBoolean(xmlXPathCompExprPtr comp,
                                           xmlXPathContextPtr ctxt);
XMLPUBFUN void xmlXPathDebugDumpObject(FILE *output, xmlXPathObjectPtr cur,
                                      int depth);
XMLPUBFUN void xmlXPathDebugDumpCompExpr(FILE *output,
                                        xmlXPathCompExprPtr comp, int depth);









/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlXPathAxis xmlXPathAxis;
typedef xmlXPathAxis *xmlXPathAxisPtr;

typedef struct _xmlXPathFunct xmlXPathFunct;
typedef xmlXPathFunct *xmlXPathFuncPtr;

typedef struct _xmlXPathType xmlXPathType;
typedef xmlXPathType *xmlXPathTypePtr;

typedef struct _xmlXPathVariable xmlXPathVariable;
typedef xmlXPathVariable *xmlXPathVariablePtr;

typedef enum{
    XPATH_EXPRESSION_OK = 0,
    XPATH_NUMBER_ERROR,
    XPATH_UNFINISHED_LITERAL_ERROR,
    XPATH_START_LITERAL_ERROR,
    XPATH_VARIABLE_REF_ERROR,
    XPATH_UNDEF_VARIABLE_ERROR,
    XPATH_INVALID_PREDICATE_ERROR,
    XPATH_EXPR_ERROR,
    XPATH_UNCLOSED_ERROR,
    XPATH_UNKNOWN_FUNC_ERROR,
    XPATH_INVALID_OPERAND,
    XPATH_INVALID_TYPE,
    XPATH_INVALID_ARITY,
    XPATH_INVALID_CTXT_SIZE,
    XPATH_INVALID_CTXT_POSITION,
    XPATH_MEMORY_ERROR,
    XPTR_SYNTAX_ERROR,
    XPTR_RESOURCE_ERROR,
    XPTR_SUB_RESOURCE_ERROR,
    XPATH_UNDEF_PREFIX_ERROR,
    XPATH_ENCODING_ERROR,
    XPATH_INVALID_CHAR_ERROR,
    XPATH_INVALID_CTXT,
    XPATH_STACK_ERROR,
    XPATH_FORBID_VARIABLE_ERROR,
    XPATH_OP_LIMIT_EXCEEDED,
    XPATH_RECURSION_LIMIT_EXCEEDED
} xmlXPathError;

typedef int (*xmlXPathConvertFunc) (xmlXPathObject *obj, int type);

typedef void (*xmlXPathEvalFunc)(xmlXPathParserContext *ctxt,
	                         int nargs);

typedef xmlXPathObject *(*xmlXPathAxisFunc) (xmlXPathParserContext *ctxt,
				 xmlXPathObject *cur);

struct _xmlXPathAxis {
    const xmlChar      *name;		/* the axis name */
    xmlXPathAxisFunc func;		/* the search function */
};

struct _xmlXPathFunct {
    const xmlChar      *name;		/* the function name */
    xmlXPathEvalFunc func;		/* the evaluation function */
};

struct _xmlXPathParserContext {
    /* the current char being parsed */
    const xmlChar *cur;
    /* the full expression */
    const xmlChar *base;

    /** error code */
    int error;

    /** the evaluation context */
    xmlXPathContext    *context;
    /** the current value */
    xmlXPathObject       *value;
    /* number of values stacked */
    int                 valueNr;
    /* max number of values stacked */
    int                valueMax;
    /* stack of values */
    xmlXPathObject **valueTab;

    /* the precompiled expression */
    xmlXPathCompExpr *comp;
    /* it this an XPointer expression */
    int xptr;
    /* used for walking preceding axis */
    xmlNode           *ancestor;

    /* always zero for compatibility */
    int              valueFrame;
};

struct _xmlXPathType {
    const xmlChar         *name;		/* the type name */
    xmlXPathConvertFunc func;		/* the conversion function */
};

struct _xmlXPathVariable {
    const xmlChar       *name;		/* the variable name */
    xmlXPathObject *value;		/* the value */
};

/* [11.1-G] end: extracted definitions */

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBVAR double xmlXPathNAN;
XMLPUBVAR double xmlXPathNINF;
XMLPUBVAR double xmlXPathPINF;
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XPATH_H__ */
