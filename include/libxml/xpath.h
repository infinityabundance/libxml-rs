/**
 * @file
 *
 * XPath API for libxml-rs
 */

#ifndef __XML_XPATH_H__
#define __XML_XPATH_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* Forward declarations */
struct _xmlXPathParserContext;
typedef struct _xmlXPathParserContext xmlXPathParserContext;
typedef xmlXPathParserContext *xmlXPathParserContextPtr;

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
                                                         const xmlChar *name);
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
    int opLimit;
    int opCount;
    int depth;
};

/* Forward declarations */
struct _xmlXPathParserContext;
typedef struct _xmlXPathParserContext xmlXPathParserContext;
typedef xmlXPathParserContext *xmlXPathParserContextPtr;

/* XPath API */
XMLPUBFUN xmlXPathContextPtr xmlXPathNewContext(xmlDocPtr doc);
XMLPUBFUN void xmlXPathFreeContext(xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr xmlXPathEvalExpression(const xmlChar *str,
                                                    xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr xmlXPathEval(const xmlChar *str,
                                          xmlXPathContextPtr ctxt);
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

#ifdef __cplusplus
}
#endif

#endif /* __XML_XPATH_H__ */
