/**
 * @file
 *
 * XPath internals API for libxml-rs (11.1-H header-surface closure).
 *
 * Upstream counterpart: include/libxml/xpathInternals.h
 * This header declares the subset of the upstream XPath-internals surface that
 * libxml-rs exports, with upstream-compatible signatures. Every declared
 * function exists in the candidate DSO (the header is honest by construction).
 * Declarations for upstream functions not yet exported are tracked as parity
 * obligations (11.1-I), not silently omitted.
 */

#ifndef __XML_XPATH_INTERNALS_H__
#define __XML_XPATH_INTERNALS_H__

#include <libxml/xmlversion.h>
#include <libxml/xpath.h>
#include <stdio.h>

/* Deprecated aliases (oracle xpathInternals.h preamble). */
#define valuePush xmlXPathValuePush
#define valuePop xmlXPathValuePop

#ifdef __cplusplus
extern "C" {
#endif

/* Function-pointer type for XPath extension functions (upstream parity).
 * Declared in libxml/xpath.h (upstream location); this header does not
 * redefine it. The candidate registers an ABI-compatible trampoline. */
#include <libxml/xpath.h>

XMLPUBFUN xmlXPathContextPtr
                xmlXPathNewContext         (xmlDocPtr doc);
XMLPUBFUN void
                xmlXPathFreeContext        (xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathEvalExpression     (const xmlChar *str,
                                            xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathEval               (const xmlChar *str,
                                            xmlXPathContextPtr ctxt);
XMLPUBFUN void
                xmlXPathFreeObject         (xmlXPathObjectPtr obj);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathObjectCopy         (xmlXPathObjectPtr val);
XMLPUBFUN xmlChar *
                xmlXPathCastToString       (xmlXPathObjectPtr val);
XMLPUBFUN double
                xmlXPathCastStringToNumber (const xmlChar *val);
XMLPUBFUN int
                xmlXPathCmpNodes           (xmlNodePtr node1,
                                            xmlNodePtr node2);
XMLPUBFUN xmlNodeSetPtr
                xmlXPathNodeSetCreate      (xmlNodePtr val);
XMLPUBFUN void
                xmlXPathFreeNodeSet        (xmlNodeSetPtr ns);
XMLPUBFUN int
                xmlXPathRegisterNs         (xmlXPathContextPtr ctxt,
                                            const xmlChar *prefix,
                                            const xmlChar *ns_uri);
XMLPUBFUN int
                xmlXPathRegisterFunc       (xmlXPathContextPtr ctxt,
                                            const xmlChar *name,
                                            xmlXPathFunction f);
XMLPUBFUN int
                xmlXPathRegisterFuncNS     (xmlXPathContextPtr ctxt,
                                            const xmlChar *name,
                                            const xmlChar *ns_uri,
                                            xmlXPathFunction f);
XMLPUBFUN int
                xmlXPathRegisterVariable   (xmlXPathContextPtr ctxt,
                                            const xmlChar *name,
                                            xmlXPathObjectPtr value);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewNodeSet         (xmlNodePtr val);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewCString         (const char *val);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewFloat           (double val);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewBoolean         (int val);

/* Upstream macros preserved for source compatibility (11.1-H). */
#define xmlXPathNodeSetIsEmpty(ns) ((ns) == NULL ? 1 : ((ns)->nodeNr == 0))
#define xmlXPathNodeSetGetLength(ns) ((ns) == NULL ? 0 : ((ns)->nodeNr))
#define xmlXPathNodeSetItem(ns, index) \
    (((ns) != NULL && (index) >= 0 && (index) < (ns)->nodeNr) ? \
     (ns)->nodeTab[(index)] : NULL)

#ifdef __cplusplus
}
#endif

#endif /* __XML_XPATH_INTERNALS_H__ */

#ifdef __cplusplus
extern "C" {
#endif
/* [11.1-L] begin: callback-family declarations extracted verbatim
 * from the oracle libxml2 2.15.3 header (only symbols exported by
 * the candidate DSO are declared). */
XMLPUBFUN void xmlXPathAddValues(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathBooleanFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathCeilingFunction(xmlXPathParserContext *ctxt, int nargs);

#define xmlXPathCheckError(ctxt)  ((ctxt)->error != XPATH_EXPRESSION_OK)

XMLPUBFUN int xmlXPathCompareValues(xmlXPathParserContext *ctxt, int inf, int strict);

XMLPUBFUN void xmlXPathConcatFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathContainsFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathCountFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void
	    xmlXPathDebugDumpCompExpr(FILE *output,
					 xmlXPathCompExpr *comp,
					 int depth);

XMLPUBFUN void
		xmlXPathDebugDumpObject	(FILE *output,
					 xmlXPathObject *cur,
					 int depth);

XMLPUBFUN xmlNodeSet *
		xmlXPathDifference		(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN xmlNodeSet *
		xmlXPathDistinct		(xmlNodeSet *nodes);

XMLPUBFUN xmlNodeSet *
		xmlXPathDistinctSorted		(xmlNodeSet *nodes);

XMLPUBFUN void xmlXPathDivValues(xmlXPathParserContext *ctxt);

#define xmlXPathEmptyNodeSet(ns)					\
    { while ((ns)->nodeNr > 0) (ns)->nodeTab[--(ns)->nodeNr] = NULL; }

XMLPUBFUN int xmlXPathEqualValues(xmlXPathParserContext *ctxt);

XMLPUBFUN void
		xmlXPathErr	(xmlXPathParserContext *ctxt,
				 int error);

XMLPUBFUN void
		xmlXPathEvalExpr		(xmlXPathParserContext *ctxt);

XMLPUBFUN int
		xmlXPathEvaluatePredicateResult (xmlXPathParserContext *ctxt,
						 xmlXPathObject *res);

XMLPUBFUN void xmlXPathFalseFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathFloorFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void
		xmlXPathFreeParserContext	(xmlXPathParserContext *ctxt);

XMLPUBFUN xmlXPathFunction
		xmlXPathFunctionLookup		(xmlXPathContext *ctxt,
						 const xmlChar *name);

XMLPUBFUN xmlXPathFunction
		xmlXPathFunctionLookupNS	(xmlXPathContext *ctxt,
						 const xmlChar *name,
						 const xmlChar *ns_uri);

#define xmlXPathGetContextNode(ctxt)	((ctxt)->context->node)

#define xmlXPathGetDocument(ctxt)	((ctxt)->context->doc)

#define xmlXPathGetError(ctxt)	  ((ctxt)->error)

XMLPUBFUN int
		xmlXPathHasSameNodes		(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN void xmlXPathIdFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN xmlNodeSet *
		xmlXPathIntersection		(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN int xmlXPathIsNodeType(const xmlChar *name);

XMLPUBFUN void xmlXPathLangFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathLastFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN xmlNodeSet *
		xmlXPathLeading			(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN xmlNodeSet *
		xmlXPathLeadingSorted		(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN void xmlXPathLocalNameFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathModValues(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathMultValues(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathNamespaceURIFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN xmlXPathObject *
		xmlXPathNewNodeSetList		(xmlNodeSet *val);

XMLPUBFUN xmlXPathParserContext *
		  xmlXPathNewParserContext	(const xmlChar *str,
						 xmlXPathContext *ctxt);

XMLPUBFUN xmlXPathObject *
		xmlXPathNewString		(const xmlChar *val);

XMLPUBFUN xmlXPathObject *
		xmlXPathNewValueTree		(xmlNode *val);

XMLPUBFUN xmlNode *xmlXPathNextAncestor(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextAncestorOrSelf(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextAttribute(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextChild(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextDescendant(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextDescendantOrSelf(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextFollowing(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextFollowingSibling(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextNamespace(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextParent(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextPreceding(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextPrecedingSibling(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNode *xmlXPathNextSelf(xmlXPathParserContext *ctxt,
			xmlNode *cur);

XMLPUBFUN xmlNodeSet *
		xmlXPathNodeLeading		(xmlNodeSet *nodes,
						 xmlNode *node);

XMLPUBFUN xmlNodeSet *
		xmlXPathNodeLeadingSorted	(xmlNodeSet *nodes,
						 xmlNode *node);

XMLPUBFUN int
		xmlXPathNodeSetAdd		(xmlNodeSet *cur,
						 xmlNode *val);

XMLPUBFUN int
		xmlXPathNodeSetAddNs		(xmlNodeSet *cur,
						 xmlNode *node,
						 xmlNs *ns);

XMLPUBFUN int
		xmlXPathNodeSetAddUnique	(xmlNodeSet *cur,
						 xmlNode *val);

XMLPUBFUN int
		xmlXPathNodeSetContains		(xmlNodeSet *cur,
						 xmlNode *val);

XMLPUBFUN void
		xmlXPathNodeSetDel		(xmlNodeSet *cur,
						 xmlNode *val);

XMLPUBFUN void xmlXPathNodeSetFreeNs(xmlNs *ns);

XMLPUBFUN xmlNodeSet *
		xmlXPathNodeSetMerge		(xmlNodeSet *val1,
						 xmlNodeSet *val2);

XMLPUBFUN void
		xmlXPathNodeSetRemove		(xmlNodeSet *cur,
						 int val);

XMLPUBFUN void
		xmlXPathNodeSetSort		(xmlNodeSet *set);

XMLPUBFUN xmlNodeSet *
		xmlXPathNodeTrailing		(xmlNodeSet *nodes,
						 xmlNode *node);

XMLPUBFUN xmlNodeSet *
		xmlXPathNodeTrailingSorted	(xmlNodeSet *nodes,
						 xmlNode *node);

XMLPUBFUN void xmlXPathNormalizeFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN int xmlXPathNotEqualValues(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathNotFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN const xmlChar *
		xmlXPathNsLookup		(xmlXPathContext *ctxt,
						 const xmlChar *prefix);

XMLPUBFUN void xmlXPathNumberFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN xmlChar *
		xmlXPathParseNCName		(xmlXPathParserContext *ctxt);

XMLPUBFUN xmlChar *
		xmlXPathParseName		(xmlXPathParserContext *ctxt);

XMLPUBFUN int
		xmlXPathPopBoolean	(xmlXPathParserContext *ctxt);

XMLPUBFUN void *
		xmlXPathPopExternal	(xmlXPathParserContext *ctxt);

XMLPUBFUN xmlNodeSet *
		xmlXPathPopNodeSet	(xmlXPathParserContext *ctxt);

XMLPUBFUN double
		xmlXPathPopNumber	(xmlXPathParserContext *ctxt);

XMLPUBFUN xmlChar *
		xmlXPathPopString	(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathPositionFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void
		xmlXPathRegisterAllFunctions	(xmlXPathContext *ctxt);

XMLPUBFUN void
	    xmlXPathRegisterFuncLookup	(xmlXPathContext *ctxt,
					 xmlXPathFuncLookupFunc f,
					 void *funcCtxt);

XMLPUBFUN void
	xmlXPathRegisterVariableLookup	(xmlXPathContext *ctxt,
					 xmlXPathVariableLookupFunc f,
					 void *data);

XMLPUBFUN int
		xmlXPathRegisterVariableNS	(xmlXPathContext *ctxt,
						 const xmlChar *name,
						 const xmlChar *ns_uri,
						 xmlXPathObject *value);

XMLPUBFUN void
		xmlXPathRegisteredFuncsCleanup	(xmlXPathContext *ctxt);

XMLPUBFUN void
		xmlXPathRegisteredNsCleanup	(xmlXPathContext *ctxt);

XMLPUBFUN void
		xmlXPathRegisteredVariablesCleanup(xmlXPathContext *ctxt);

#define xmlXPathReturnBoolean(ctxt, val)				\
    valuePush((ctxt), xmlXPathNewBoolean(val))

#define xmlXPathReturnEmptyNodeSet(ctxt)				\
    valuePush((ctxt), xmlXPathNewNodeSet(NULL))

#define xmlXPathReturnEmptyString(ctxt)					\
    valuePush((ctxt), xmlXPathNewCString(""))

#define xmlXPathReturnExternal(ctxt, val)				\
    valuePush((ctxt), xmlXPathWrapExternal(val))

#define xmlXPathReturnFalse(ctxt)  xmlXPathReturnBoolean((ctxt), 0)

#define xmlXPathReturnNodeSet(ctxt, ns)					\
    valuePush((ctxt), xmlXPathWrapNodeSet(ns))

#define xmlXPathReturnNumber(ctxt, val)					\
    valuePush((ctxt), xmlXPathNewFloat(val))

#define xmlXPathReturnString(ctxt, str)					\
    valuePush((ctxt), xmlXPathWrapString(str))

#define xmlXPathReturnTrue(ctxt)   xmlXPathReturnBoolean((ctxt), 1)

XMLPUBFUN void
		xmlXPathRoot			(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathRoundFunction(xmlXPathParserContext *ctxt, int nargs);

#define xmlXPathSetArityError(ctxt)					\
    xmlXPathSetError((ctxt), XPATH_INVALID_ARITY)

#define xmlXPathSetError(ctxt, err)					\
    { xmlXPatherror((ctxt), __FILE__, __LINE__, (err));			\
      if ((ctxt) != NULL) (ctxt)->error = (err); }

#define xmlXPathSetTypeError(ctxt)					\
    xmlXPathSetError((ctxt), XPATH_INVALID_TYPE)

#define xmlXPathStackIsExternal(ctxt)					\
	((ctxt->value != NULL) && (ctxt->value->type == XPATH_USERS))

#define xmlXPathStackIsNodeSet(ctxt)					\
    (((ctxt)->value != NULL)						\
     && (((ctxt)->value->type == XPATH_NODESET)				\
         || ((ctxt)->value->type == XPATH_XSLT_TREE)))

XMLPUBFUN void xmlXPathStartsWithFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN double
		xmlXPathStringEvalNumber	(const xmlChar *str);

XMLPUBFUN void xmlXPathStringFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathStringLengthFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathSubValues(xmlXPathParserContext *ctxt);

XMLPUBFUN void xmlXPathSubstringAfterFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathSubstringBeforeFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathSubstringFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathSumFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN xmlNodeSet *
		xmlXPathTrailing		(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN xmlNodeSet *
		xmlXPathTrailingSorted		(xmlNodeSet *nodes1,
						 xmlNodeSet *nodes2);

XMLPUBFUN void xmlXPathTranslateFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathTrueFunction(xmlXPathParserContext *ctxt, int nargs);

XMLPUBFUN void xmlXPathValueFlipSign(xmlXPathParserContext *ctxt);

XMLPUBFUN xmlXPathObject *
		xmlXPathValuePop		(xmlXPathParserContext *ctxt);

XMLPUBFUN int
		xmlXPathValuePush		(xmlXPathParserContext *ctxt,
						 xmlXPathObject *value);

XMLPUBFUN xmlXPathObject *
		xmlXPathVariableLookup		(xmlXPathContext *ctxt,
						 const xmlChar *name);

XMLPUBFUN xmlXPathObject *
		xmlXPathVariableLookupNS	(xmlXPathContext *ctxt,
						 const xmlChar *name,
						 const xmlChar *ns_uri);

XMLPUBFUN xmlXPathObject *
		xmlXPathWrapCString		(char * val);

XMLPUBFUN xmlXPathObject *
		xmlXPathWrapExternal		(void *val);

XMLPUBFUN xmlXPathObject *
		xmlXPathWrapNodeSet		(xmlNodeSet *val);

XMLPUBFUN xmlXPathObject *
		xmlXPathWrapString		(xmlChar *val);

XMLPUBFUN void
		xmlXPatherror	(xmlXPathParserContext *ctxt,
				 const char *file,
				 int line,
				 int no);

/* [11.1-L] end: extracted declarations */
#ifdef __cplusplus
}
#endif

