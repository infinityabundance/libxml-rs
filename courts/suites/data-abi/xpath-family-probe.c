/*
 * XPATH-001 — differential court for the xmlXPath* family closure
 * (11.1-I XPath family).
 *
 * Exercises the complete exported xmlXPath* surface (including the symbols
 * that are exported from the DSO but no longer declared in the 2.15 public
 * header) and prints deterministic observations. The court requires
 * byte-identical stdout between the system libxml2 2.15.3 and the candidate.
 *
 * Only values (names, types, numbers, strings) are printed — never pointers.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/xmlstring.h>

/* ── Externs for DSO-exported symbols removed from the 2.15 public header ── */
extern xmlXPathObjectPtr xmlXPathNewString(const xmlChar *val);
extern xmlXPathObjectPtr xmlXPathNewFloat(double val);
extern xmlXPathObjectPtr xmlXPathNewBoolean(int val);
extern xmlXPathObjectPtr xmlXPathNewNodeSet(xmlNodePtr val);
extern xmlXPathObjectPtr xmlXPathNewNodeSetList(xmlNodeSetPtr val);
extern xmlXPathObjectPtr xmlXPathWrapString(xmlChar *val);
extern xmlNodeSetPtr xmlXPathNodeSetCreate(xmlNodePtr val);
extern xmlXPathObjectPtr xmlXPathWrapExternal(void *val);
extern int xmlXPathNodeSetAdd(xmlNodeSetPtr cur, xmlNodePtr val);
extern xmlChar *xmlXPathCastToString(xmlXPathObjectPtr val);
extern xmlChar *xmlXPathCastNumberToString(double val);
extern xmlXPathParserContextPtr xmlXPathNewParserContext(const xmlChar *str,
                                                        xmlXPathContextPtr ctxt);
extern void xmlXPathFreeParserContext(xmlXPathParserContextPtr ctxt);
extern xmlXPathObjectPtr xmlXPathValuePop(xmlXPathParserContextPtr ctxt);
extern xmlXPathObjectPtr xmlXPathValuePush(xmlXPathParserContextPtr ctxt,
                                          xmlXPathObjectPtr value);
extern double xmlXPathPopNumber(xmlXPathParserContextPtr ctxt);
extern int xmlXPathPopBoolean(xmlXPathParserContextPtr ctxt);
extern xmlChar *xmlXPathPopString(xmlXPathParserContextPtr ctxt);
extern xmlNodeSetPtr xmlXPathPopNodeSet(xmlXPathParserContextPtr ctxt);
extern void *xmlXPathPopExternal(xmlXPathParserContextPtr ctxt);
extern void xmlXPathAddValues(xmlXPathParserContextPtr ctxt);
extern void xmlXPathSubValues(xmlXPathParserContextPtr ctxt);
extern void xmlXPathMultValues(xmlXPathParserContextPtr ctxt);
extern void xmlXPathDivValues(xmlXPathParserContextPtr ctxt);
extern void xmlXPathModValues(xmlXPathParserContextPtr ctxt);
extern void xmlXPathValueFlipSign(xmlXPathParserContextPtr ctxt);
extern int xmlXPathEqualValues(xmlXPathParserContextPtr ctxt);
extern int xmlXPathNotEqualValues(xmlXPathParserContextPtr ctxt);
extern int xmlXPathCompareValues(xmlXPathParserContextPtr ctxt, int inf,
                                 int strict);
extern xmlChar *xmlXPathParseName(xmlXPathParserContextPtr ctxt);
extern xmlChar *xmlXPathParseNCName(xmlXPathParserContextPtr ctxt);
extern void xmlXPathRoot(xmlXPathParserContextPtr ctxt);
extern void xmlXPathEvalExpr(xmlXPathParserContextPtr ctxt);
extern int xmlXPathEvalPredicate(xmlXPathContextPtr ctxt,
                                 xmlXPathObjectPtr res);
extern int xmlXPathEvaluatePredicateResult(xmlXPathParserContextPtr ctxt,
                                           xmlXPathObjectPtr res);
extern xmlNodePtr xmlXPathNextChild(xmlXPathParserContextPtr ctxt,
                                   xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextFollowingSibling(xmlXPathParserContextPtr ctxt,
                                               xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextPrecedingSibling(xmlXPathParserContextPtr ctxt,
                                               xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextParent(xmlXPathParserContextPtr ctxt,
                                    xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextAncestor(xmlXPathParserContextPtr ctxt,
                                      xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextAncestorOrSelf(xmlXPathParserContextPtr ctxt,
                                             xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextSelf(xmlXPathParserContextPtr ctxt,
                                  xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextDescendant(xmlXPathParserContextPtr ctxt,
                                        xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextDescendantOrSelf(xmlXPathParserContextPtr ctxt,
                                               xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextFollowing(xmlXPathParserContextPtr ctxt,
                                       xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextPreceding(xmlXPathParserContextPtr ctxt,
                                       xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextAttribute(xmlXPathParserContextPtr ctxt,
                                       xmlNodePtr cur);
extern xmlNodePtr xmlXPathNextNamespace(xmlXPathParserContextPtr ctxt,
                                       xmlNodePtr cur);
extern void xmlXPathBooleanFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathNotFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathTrueFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathFalseFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathNumberFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathSumFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathFloorFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathCeilingFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathRoundFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathLastFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathPositionFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathCountFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathLocalNameFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathNamespaceURIFunction(xmlXPathParserContextPtr ctxt,
                                         int nargs);
extern void xmlXPathStringFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathStringLengthFunction(xmlXPathParserContextPtr ctxt,
                                         int nargs);
extern void xmlXPathConcatFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathContainsFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathStartsWithFunction(xmlXPathParserContextPtr ctxt,
                                       int nargs);
extern void xmlXPathSubstringFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathSubstringBeforeFunction(xmlXPathParserContextPtr ctxt,
                                            int nargs);
extern void xmlXPathSubstringAfterFunction(xmlXPathParserContextPtr ctxt,
                                           int nargs);
extern void xmlXPathNormalizeFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathTranslateFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathLangFunction(xmlXPathParserContextPtr ctxt, int nargs);
extern void xmlXPathRegisterAllFunctions(xmlXPathContextPtr ctxt);
extern xmlXPathFunction xmlXPathFunctionLookup(xmlXPathContextPtr ctxt,
                                              const xmlChar *name);
extern xmlXPathFunction xmlXPathFunctionLookupNS(xmlXPathContextPtr ctxt,
                                                const xmlChar *name,
                                                const xmlChar *ns_uri);
extern xmlXPathObjectPtr xmlXPathVariableLookup(xmlXPathContextPtr ctxt,
                                               const xmlChar *name);
extern xmlXPathObjectPtr xmlXPathVariableLookupNS(xmlXPathContextPtr ctxt,
                                                 const xmlChar *name,
                                                 const xmlChar *ns_uri);
extern const xmlChar *xmlXPathNsLookup(xmlXPathContextPtr ctxt,
                                      const xmlChar *prefix);
extern int xmlXPathRegisterVariableNS(xmlXPathContextPtr ctxt,
                                     const xmlChar *name,
                                     const xmlChar *ns_uri,
                                     xmlXPathObjectPtr value);
extern int xmlXPathRegisterNs(xmlXPathContextPtr ctxt,
                             const xmlChar *prefix, const xmlChar *ns_uri);
extern int xmlXPathRegisterVariable(xmlXPathContextPtr ctxt,
                                   const xmlChar *name,
                                   xmlXPathObjectPtr value);
extern int xmlXPathRegisterFunc(xmlXPathContextPtr ctxt, const xmlChar *name,
                               xmlXPathFunction f);
extern int xmlXPathRegisterFuncNS(xmlXPathContextPtr ctxt,
                                 const xmlChar *name, const xmlChar *ns_uri,
                                 xmlXPathFunction f);
extern void xmlXPathRegisterFuncLookup(xmlXPathContextPtr ctxt,
                                      xmlXPathFuncLookupFunc f,
                                      void *funcCtxt);
extern void xmlXPathRegisterVariableLookup(xmlXPathContextPtr ctxt,
                                          xmlXPathVariableLookupFunc f,
                                          void *data);
extern void xmlXPathRegisteredFuncsCleanup(xmlXPathContextPtr ctxt);
extern void xmlXPathRegisteredNsCleanup(xmlXPathContextPtr ctxt);
extern void xmlXPathRegisteredVariablesCleanup(xmlXPathContextPtr ctxt);
extern xmlXPathObjectPtr xmlXPathCompiledEval(xmlXPathCompExprPtr comp,
                                             xmlXPathContextPtr ctx);
extern int xmlXPathCompiledEvalToBoolean(xmlXPathCompExprPtr comp,
                                        xmlXPathContextPtr ctxt);
extern long xmlXPathOrderDocElems(xmlDocPtr doc);
extern void xmlXPathDebugDumpObject(FILE *output, xmlXPathObjectPtr cur,
                                    int depth);
extern void xmlXPathDebugDumpCompExpr(FILE *output, xmlXPathCompExprPtr comp,
                                      int depth);

/* ── helpers ── */
static void print_string(const char *label, xmlChar *s) {
    printf("%s=%s\n", label, s ? (char *)s : "(null)");
}

static void print_obj(const char *label, xmlXPathObjectPtr obj) {
    if (!obj) { printf("%s=(null)\n", label); return; }
    switch (obj->type) {
    case XPATH_BOOLEAN:
        printf("%s=bool:%d\n", label, obj->boolval);
        break;
    case XPATH_NUMBER:
        printf("%s=num:%g\n", label, obj->floatval);
        break;
    case XPATH_STRING:
        printf("%s=str:%s\n", label,
               obj->stringval ? (char *)obj->stringval : "(null)");
        break;
    case XPATH_NODESET:
        printf("%s=ns:%d\n", label,
               obj->nodesetval ? obj->nodesetval->nodeNr : -1);
        break;
    default:
        printf("%s=type:%d\n", label, obj->type);
        break;
    }
}

/* C extension function used for RegisterFunc / lookup tests */
static void triple_func(xmlXPathParserContextPtr ctxt, int nargs) {
    (void)nargs;
    double v = xmlXPathPopNumber(ctxt);
    xmlXPathValuePush(ctxt, xmlXPathNewFloat(v * 3.0));
}

/* C extension function used for ns-scoped registration */
static void ns_func(xmlXPathParserContextPtr ctxt, int nargs) {
    (void)nargs;
    xmlXPathValuePush(ctxt, xmlXPathNewBoolean(7));
}

static xmlXPathFunction my_func_lookup(void *data, const xmlChar *name,
                                       const xmlChar *ns_uri) {
    (void)data; (void)ns_uri;
    if (name && strcmp((char *)name, "dyn") == 0)
        return triple_func;
    return NULL;
}

static xmlXPathObjectPtr my_var_lookup(void *data, const xmlChar *name,
                                       const xmlChar *ns_uri) {
    (void)data; (void)name; (void)ns_uri;
    return xmlXPathNewFloat(42.0);
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    xmlInitParser();
    xmlDocPtr doc = xmlReadMemory(
        "<root a=\"1\"><b id=\"b1\">t1</b><b id=\"b2\">t2</b><c/></root>",
        (int)strlen("<root a=\"1\"><b id=\"b1\">t1</b><b id=\"b2\">t2</b><c/></root>"), "x.xml", NULL, 0);
    xmlNodePtr root = xmlDocGetRootElement(doc);
    xmlNodePtr b1 = root->children;
    xmlNodePtr b2 = b1->next;
    xmlNodePtr c = b2->next;

    xmlDocPtr doc2 = xmlReadMemory("<root><a>1</a><a>2</a></root>",
                                  (int)strlen("<root><a>1</a><a>2</a></root>"),
                                  "y.xml", NULL, 0);
    xmlNodePtr r2 = xmlDocGetRootElement(doc2);
    xmlNodePtr a1 = r2->children;
    xmlNodePtr a2 = a1->next;

    xmlDocPtr doc3 = xmlReadMemory(
        "<root xmlns=\"urn:def\" xmlns:p=\"urn:p\"><child xmlns:q=\"urn:q\"/></root>",
        (int)strlen("<root xmlns=\"urn:def\" xmlns:p=\"urn:p\"><child xmlns:q=\"urn:q\"/></root>"),
        "z.xml", NULL, 0);
    xmlNodePtr r3 = xmlDocGetRootElement(doc3);
    xmlNodePtr child = r3->children;

    xmlDocPtr doc4 = xmlReadMemory(
        "<root xml:lang=\"en-US\"><leaf/></root>",
        (int)strlen("<root xml:lang=\"en-US\"><leaf/></root>"), "w.xml", NULL, 0);
    xmlNodePtr r4 = xmlDocGetRootElement(doc4);

    xmlDocPtr doc5 = xmlReadMemory(
        "<root><m:item xmlns:m=\"urn:mid\">5</m:item><item>2</item></root>",
        (int)strlen("<root><m:item xmlns:m=\"urn:mid\">5</m:item><item>2</item></root>"),
        "v.xml", NULL, 0);
    xmlNodePtr r5 = xmlDocGetRootElement(doc5);
    xmlNodePtr mitem = r5->children;

    xmlXPathContextPtr ctxt = xmlXPathNewContext(doc);
    xmlXPathParserContextPtr pctxt;

    /* ── 1. object construction / casts ── */
    print_obj("newstring", xmlXPathNewString(BAD_CAST "abc"));
    print_obj("newfloat", xmlXPathNewFloat(1.5));
    print_obj("newbool", xmlXPathNewBoolean(1));
    print_obj("newnodeset", xmlXPathNewNodeSet(b1));
    print_string("castnum1", xmlXPathCastNumberToString(1.5));
    print_string("castnum2", xmlXPathCastNumberToString(2.0));
    print_string("castnum3", xmlXPathCastNumberToString(-3.25));
    print_string("caststr", xmlXPathCastToString(xmlXPathNewFloat(7.0)));

    /* ── 2. parser context + value stack ── */
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(5.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(3.0));
    xmlXPathAddValues(pctxt);
    print_obj("add", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(9.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    xmlXPathSubValues(pctxt);
    print_obj("sub", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(6.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(7.0));
    xmlXPathMultValues(pctxt);
    print_obj("mult", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(10.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    xmlXPathDivValues(pctxt);
    print_obj("div", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(11.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    xmlXPathModValues(pctxt);
    print_obj("mod", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.5));
    xmlXPathValueFlipSign(pctxt);
    print_obj("flip", xmlXPathValuePop(pctxt));
    /* string operand converts to number */
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.0));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "4"));
    xmlXPathAddValues(pctxt);
    print_obj("addstr", xmlXPathValuePop(pctxt));
    /* typed pops */
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(3.25));
    printf("popnum=%g\n", xmlXPathPopNumber(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewBoolean(1));
    printf("popbool=%d\n", xmlXPathPopBoolean(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "xyz"));
    print_string("popstr", xmlXPathPopString(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(b1));
    print_obj("popns", xmlXPathNewNodeSetList(xmlXPathPopNodeSet(pctxt)));
    xmlXPathValuePush(pctxt, xmlXPathWrapExternal((void *)0x1));
    printf("popext=%d\n", xmlXPathPopExternal(pctxt) != NULL);
    xmlXPathFreeParserContext(pctxt);

    /* equality / comparison */
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    printf("eq=%d\n", xmlXPathEqualValues(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(1.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.0));
    printf("neq=%d\n", xmlXPathNotEqualValues(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(3.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(5.0));
    printf("lt=%d\n", xmlXPathCompareValues(pctxt, 1, 1));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(5.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(5.0));
    printf("le=%d\n", xmlXPathCompareValues(pctxt, 1, 0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(9.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.0));
    printf("gt=%d\n", xmlXPathCompareValues(pctxt, 0, 1));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.0));
    printf("ge=%d\n", xmlXPathCompareValues(pctxt, 0, 0));
    /* string vs number equality */
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "4.0"));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    printf("eqstrnum=%d\n", xmlXPathEqualValues(pctxt));
    /* node-set vs node-set (string equality, distinct nodes) */
    xmlNodeSetPtr nsA = xmlXPathNodeSetCreate(a1);
    xmlXPathNodeSetAdd(nsA, a2);
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSetList(nsA));
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(b1));
    printf("eqns=%d\n", xmlXPathEqualValues(pctxt));
    /* node-set vs number */
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(a1));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(1.0));
    printf("eqnsnum=%d\n", xmlXPathEqualValues(pctxt));
    /* node-set vs string */
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(a1));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "1"));
    printf("eqnsstr=%d\n", xmlXPathEqualValues(pctxt));
    /* node-set != number */
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(a2));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(1.0));
    printf("neqnsnum=%d\n", xmlXPathNotEqualValues(pctxt));
    /* NaN / Inf equality */
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(0.0 / 0.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(0.0 / 0.0));
    printf("eqnan=%d\n", xmlXPathEqualValues(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(1.0 / 0.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(1.0 / 0.0));
    printf("eqinf=%d\n", xmlXPathEqualValues(pctxt));
    xmlXPathFreeParserContext(pctxt);

    /* ── 3. ParseName / ParseNCName / Root / EvalExpr ── */
    pctxt = xmlXPathNewParserContext(BAD_CAST "one two", ctxt);
    print_string("parsename", xmlXPathParseName(pctxt));
    xmlXPathFreeParserContext(pctxt);
    pctxt = xmlXPathNewParserContext(BAD_CAST "one:two", ctxt);
    print_string("parsencname", xmlXPathParseNCName(pctxt));
    xmlXPathFreeParserContext(pctxt);
    pctxt = xmlXPathNewParserContext(BAD_CAST "one two", ctxt);
    print_string("parsename2", xmlXPathParseName(pctxt));
    xmlXPathFreeParserContext(pctxt);

    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    xmlXPathRoot(pctxt);
    print_obj("root", xmlXPathValuePop(pctxt));
    xmlXPathFreeParserContext(pctxt);

    pctxt = xmlXPathNewParserContext(BAD_CAST "1 + 2 * 3", ctxt);
    xmlXPathEvalExpr(pctxt);
    printf("evalexpr_err=%d\n", pctxt->error);
    print_obj("evalexpr", xmlXPathValuePop(pctxt));
    xmlXPathFreeParserContext(pctxt);
    pctxt = xmlXPathNewParserContext(BAD_CAST "count(/root/b)", ctxt);
    xmlXPathEvalExpr(pctxt);
    print_obj("evalexpr2", xmlXPathValuePop(pctxt));
    xmlXPathFreeParserContext(pctxt);

    /* predicates */
    xmlXPathObjectPtr pbool = xmlXPathNewBoolean(1);
    xmlXPathObjectPtr pnum = xmlXPathNewFloat(2.0);
    xmlXPathObjectPtr pstr = xmlXPathNewString(BAD_CAST "");
    ctxt->proximityPosition = 2;
    printf("predbool=%d\n", xmlXPathEvalPredicate(ctxt, pbool));
    printf("prednum=%d\n", xmlXPathEvalPredicate(ctxt, pnum));
    printf("predstr=%d\n", xmlXPathEvalPredicate(ctxt, pstr));
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    printf("predresnum=%d\n",
           xmlXPathEvaluatePredicateResult(pctxt, pnum));
    xmlXPathFreeParserContext(pctxt);
    xmlXPathFreeObject(pbool);
    xmlXPathFreeObject(pnum);
    xmlXPathFreeObject(pstr);

    /* ── 4. axes ── */
    xmlXPathSetContextNode(root, ctxt);
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    printf("self=%s\n", xmlXPathNextSelf(pctxt, NULL)->name);
    printf("self2=%d\n", xmlXPathNextSelf(pctxt, root) == NULL);
    printf("child1=%s\n", xmlXPathNextChild(pctxt, NULL)->name);
    printf("child2=%s\n", xmlXPathNextChild(pctxt, b1)->name);
    printf("child3=%s\n", xmlXPathNextChild(pctxt, b2)->name);
    printf("child4=%d\n", xmlXPathNextChild(pctxt, c) == NULL);
    printf("foll1=%s\n", xmlXPathNextFollowingSibling(pctxt, b1)->name);
    printf("foll2=%s\n", xmlXPathNextFollowingSibling(pctxt, b2)->name);
    printf("foll3=%d\n", xmlXPathNextFollowingSibling(pctxt, c) == NULL);
    printf("desc1=%s\n", xmlXPathNextDescendant(pctxt, NULL)->name);
    printf("desc2=%s\n", xmlXPathNextDescendant(pctxt, b1)->name);
    xmlXPathSetContextNode(c, ctxt);
    printf("prevsib1=%s\n", xmlXPathNextPrecedingSibling(pctxt, NULL)->name);
    printf("prevsib2=%s\n", xmlXPathNextPrecedingSibling(pctxt, b2)->name);
    printf("prevsib3=%d\n", xmlXPathNextPrecedingSibling(pctxt, b1) == NULL);
    printf("parent=%s\n", xmlXPathNextParent(pctxt, NULL)->name);
    printf("ancestorself=%s\n", xmlXPathNextAncestorOrSelf(pctxt, NULL)->name);
    xmlXPathSetContextNode(root, ctxt);
    printf("anc=%d\n", xmlXPathNextAncestor(pctxt, NULL) == (xmlNodePtr)doc);
    printf("descorself1=%s\n", xmlXPathNextDescendantOrSelf(pctxt, NULL)->name);
    printf("descorself2=%s\n", xmlXPathNextDescendantOrSelf(pctxt, root)->name);
    xmlXPathSetContextNode(b1, ctxt);
    printf("follow1=%s\n", xmlXPathNextFollowing(pctxt, NULL)->name);
    xmlXPathSetContextNode(c, ctxt);
    printf("prec1=%s\n", xmlXPathNextPreceding(pctxt, NULL)->name);
    printf("prec2=%s\n", xmlXPathNextPreceding(pctxt, b2)->name);
    xmlXPathSetContextNode(root, ctxt);
    printf("attr1=%s\n", xmlXPathNextAttribute(pctxt, NULL)->name);
    printf("attr2=%d\n", xmlXPathNextAttribute(pctxt, (xmlNodePtr)root->properties) == NULL);
    xmlXPathFreeParserContext(pctxt);

    /* namespace axis */
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    {
        xmlXPathContextPtr c3 = xmlXPathNewContext(doc3);
        xmlXPathSetContextNode(child, c3);
        xmlXPathParserContextPtr p3 = xmlXPathNewParserContext(BAD_CAST "", c3);
        xmlNodePtr n = xmlXPathNextNamespace(p3, NULL);
        int i = 0;
        while (n && i < 8) {
            xmlNsPtr ns = (xmlNsPtr)n;
            printf("ns%d=%s|%s\n", i,
                   ns->prefix ? (char *)ns->prefix : "(default)",
                   ns->href ? (char *)ns->href : "(null)");
            n = xmlXPathNextNamespace(p3, n);
            i++;
        }
        printf("nsend=%d\n", n == NULL);
        xmlXPathFreeParserContext(p3);
        xmlXPathFreeContext(c3);
    }
    xmlXPathFreeParserContext(pctxt);

    /* ── 5. core function shims ── */
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "hello"));
    xmlXPathBooleanFunction(pctxt, 1);
    print_obj("f_boolean", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewBoolean(1));
    xmlXPathNotFunction(pctxt, 1);
    print_obj("f_not", xmlXPathValuePop(pctxt));
    xmlXPathTrueFunction(pctxt, 0);
    print_obj("f_true", xmlXPathValuePop(pctxt));
    xmlXPathFalseFunction(pctxt, 0);
    print_obj("f_false", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "3.5"));
    xmlXPathNumberFunction(pctxt, 1);
    print_obj("f_number", xmlXPathValuePop(pctxt));
    xmlXPathSetContextNode(r2, ctxt);
    xmlXPathNumberFunction(pctxt, 0);
    print_obj("f_number0", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSetList(nsA));
    xmlXPathSumFunction(pctxt, 1);
    print_obj("f_sum", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.7));
    xmlXPathFloorFunction(pctxt, 1);
    print_obj("f_floor", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.1));
    xmlXPathCeilingFunction(pctxt, 1);
    print_obj("f_ceil", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.5));
    xmlXPathRoundFunction(pctxt, 1);
    print_obj("f_round", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(-0.5));
    xmlXPathRoundFunction(pctxt, 1);
    print_obj("f_roundneg", xmlXPathValuePop(pctxt));
    ctxt->contextSize = 3;
    ctxt->proximityPosition = 2;
    xmlXPathLastFunction(pctxt, 0);
    print_obj("f_last", xmlXPathValuePop(pctxt));
    xmlXPathPositionFunction(pctxt, 0);
    print_obj("f_position", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSetList(nsA));
    xmlXPathCountFunction(pctxt, 1);
    print_obj("f_count", xmlXPathValuePop(pctxt));
    xmlXPathSetContextNode(root, ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(mitem));
    xmlXPathLocalNameFunction(pctxt, 1);
    print_obj("f_localname", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewNodeSet(mitem));
    xmlXPathNamespaceURIFunction(pctxt, 1);
    print_obj("f_nsuri", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(1.5));
    xmlXPathStringFunction(pctxt, 1);
    print_obj("f_string", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "héllo"));
    xmlXPathStringLengthFunction(pctxt, 1);
    print_obj("f_strlen", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "a"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "b"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "c"));
    xmlXPathConcatFunction(pctxt, 3);
    print_obj("f_concat", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "hello"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "ell"));
    xmlXPathContainsFunction(pctxt, 2);
    print_obj("f_contains", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "hello"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "he"));
    xmlXPathStartsWithFunction(pctxt, 2);
    print_obj("f_startswith", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "12345"));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(2.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(3.0));
    xmlXPathSubstringFunction(pctxt, 3);
    print_obj("f_substr", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "12345"));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(0.0));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(3.0));
    xmlXPathSubstringFunction(pctxt, 3);
    print_obj("f_substr2", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "1999/04/01"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "/"));
    xmlXPathSubstringBeforeFunction(pctxt, 2);
    print_obj("f_subbefore", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "1999/04/01"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "/"));
    xmlXPathSubstringAfterFunction(pctxt, 2);
    print_obj("f_subafter", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "  a   b\tc  "));
    xmlXPathNormalizeFunction(pctxt, 1);
    print_obj("f_normalize", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "bar"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "abc"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "ABC"));
    xmlXPathTranslateFunction(pctxt, 3);
    print_obj("f_translate", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "--aaa--"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "abc-"));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "ABC"));
    xmlXPathTranslateFunction(pctxt, 3);
    print_obj("f_translate2", xmlXPathValuePop(pctxt));
    /* lang() */
    xmlXPathSetContextNode(r4, ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "en"));
    xmlXPathLangFunction(pctxt, 1);
    print_obj("f_lang_en", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "fr"));
    xmlXPathLangFunction(pctxt, 1);
    print_obj("f_lang_fr", xmlXPathValuePop(pctxt));
    xmlXPathFreeParserContext(pctxt);

    /* ── 6. compiled expressions / node eval ── */
    xmlXPathCompExprPtr comp = xmlXPathCompile(BAD_CAST "2 + 3");
    print_obj("compileval", xmlXPathCompiledEval(comp, ctxt));
    printf("compiledbool=%d\n",
           xmlXPathCompiledEvalToBoolean(comp, ctxt));
    xmlXPathFreeCompExpr(comp);
    comp = xmlXPathCtxtCompile(ctxt, BAD_CAST "5 * 4");
    print_obj("ctxtcompile", xmlXPathCompiledEval(comp, ctxt));
    xmlXPathFreeCompExpr(comp);
    xmlXPathSetContextNode(root, ctxt);
    print_obj("nodeeval", xmlXPathNodeEval(root, BAD_CAST "count(b)", ctxt));
    printf("setctx=%d\n", xmlXPathSetContextNode(root, ctxt));
    printf("setctxbad=%d\n", xmlXPathSetContextNode(r2->children, ctxt));
    printf("orderdoc=%ld\n", xmlXPathOrderDocElems(doc));
    printf("orderdocnull=%ld\n", xmlXPathOrderDocElems(NULL));
    printf("setcache=%d\n", xmlXPathContextSetCache(ctxt, 1, 0, 0));
    xmlXPathRegisterAllFunctions(ctxt);

    /* ── 7. registration / lookup ── */
    printf("regns=%d\n", xmlXPathRegisterNs(ctxt, BAD_CAST "m", BAD_CAST "urn:mid"));
    printf("nslookup=%s\n", xmlXPathNsLookup(ctxt, BAD_CAST "m")
               ? (char *)xmlXPathNsLookup(ctxt, BAD_CAST "m") : "(null)");
    printf("nsxml=%s\n", xmlXPathNsLookup(ctxt, BAD_CAST "xml"));
    printf("nsmiss=%d\n", xmlXPathNsLookup(ctxt, BAD_CAST "zzz") == NULL);
    xmlXPathRegisteredNsCleanup(ctxt);
    printf("nsafter=%d\n", xmlXPathNsLookup(ctxt, BAD_CAST "m") == NULL);

    printf("regvar=%d\n", xmlXPathRegisterVariable(ctxt, BAD_CAST "v",
                                                   xmlXPathNewFloat(7.0)));
    print_obj("varlookup", xmlXPathVariableLookup(ctxt, BAD_CAST "v"));
    xmlXPathRegisteredVariablesCleanup(ctxt);
    printf("varafter=%d\n", xmlXPathVariableLookup(ctxt, BAD_CAST "v") == NULL);

    printf("regvarns=%d\n", xmlXPathRegisterVariableNS(ctxt, BAD_CAST "w",
                                                       BAD_CAST "urn:w",
                                                       xmlXPathNewFloat(9.0)));
    print_obj("varlookupns", xmlXPathVariableLookupNS(ctxt, BAD_CAST "w",
                                                     BAD_CAST "urn:w"));
    printf("varlookupnsmiss=%d\n",
           xmlXPathVariableLookupNS(ctxt, BAD_CAST "w", BAD_CAST "urn:x") == NULL);
    printf("varlookupplain=%d\n",
           xmlXPathVariableLookup(ctxt, BAD_CAST "w") == NULL);
    xmlXPathRegisteredVariablesCleanup(ctxt);

    printf("regfunc=%d\n", xmlXPathRegisterFunc(ctxt, BAD_CAST "triple",
                                                triple_func));
    printf("funclookup=%d\n", xmlXPathFunctionLookup(ctxt, BAD_CAST "triple")
               != NULL);
    printf("funclookupcore=%d\n",
           xmlXPathFunctionLookup(ctxt, BAD_CAST "boolean") != NULL);
    printf("funclookupmiss=%d\n",
           xmlXPathFunctionLookup(ctxt, BAD_CAST "nosuch") == NULL);
    /* call the registered function through a parser context */
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    triple_func(pctxt, 1);
    print_obj("triple", xmlXPathValuePop(pctxt));
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(4.0));
    xmlXPathFunctionLookup(ctxt, BAD_CAST "triple")(pctxt, 1);
    print_obj("triple_lookup", xmlXPathValuePop(pctxt));
    /* standard function through lookup */
    xmlXPathValuePush(pctxt, xmlXPathNewString(BAD_CAST "x"));
    xmlXPathFunctionLookup(ctxt, BAD_CAST "boolean")(pctxt, 1);
    print_obj("boolean_lookup", xmlXPathValuePop(pctxt));
    xmlXPathFreeParserContext(pctxt);
    printf("regfuncns=%d\n", xmlXPathRegisterFuncNS(ctxt, BAD_CAST "mark",
                                                    BAD_CAST "urn:f",
                                                    ns_func));
    printf("funclookupns=%d\n",
           xmlXPathFunctionLookupNS(ctxt, BAD_CAST "mark", BAD_CAST "urn:f")
               != NULL);
    printf("funclookupnswrong=%d\n",
           xmlXPathFunctionLookupNS(ctxt, BAD_CAST "mark", BAD_CAST "urn:g")
               == NULL);
    xmlXPathRegisteredFuncsCleanup(ctxt);
    printf("funcafter=%d\n",
           xmlXPathFunctionLookup(ctxt, BAD_CAST "triple") == NULL);

    xmlXPathRegisterFuncLookup(ctxt, my_func_lookup, NULL);
    printf("funclookupext=%d\n",
           xmlXPathFunctionLookup(ctxt, BAD_CAST "dyn") != NULL);
    pctxt = xmlXPathNewParserContext(BAD_CAST "", ctxt);
    xmlXPathValuePush(pctxt, xmlXPathNewFloat(5.0));
    xmlXPathFunctionLookup(ctxt, BAD_CAST "dyn")(pctxt, 1);
    print_obj("dyn", xmlXPathValuePop(pctxt));
    xmlXPathFreeParserContext(pctxt);

    xmlXPathRegisterVariableLookup(ctxt, my_var_lookup, NULL);
    print_obj("var_lookup_cb", xmlXPathVariableLookup(ctxt, BAD_CAST "any"));
    print_obj("var_lookup_ns_cb",
              xmlXPathVariableLookupNS(ctxt, BAD_CAST "any", BAD_CAST "urn:q"));

    xmlXPathSetErrorHandler(ctxt, NULL, NULL);

    /* ── 8. debug dumps ── */
    xmlXPathDebugDumpObject(stdout, xmlXPathNewBoolean(1), 0);
    xmlXPathDebugDumpObject(stdout, xmlXPathNewFloat(1.5), 0);
    xmlXPathDebugDumpObject(stdout, xmlXPathNewString(BAD_CAST "hi"), 0);
    xmlXPathDebugDumpObject(stdout, NULL, 0);
    xmlXPathDebugDumpCompExpr(stdout, NULL, 0);

    /* ── cleanup ── */
    xmlXPathFreeContext(ctxt);
    xmlFreeDoc(doc);
    xmlFreeDoc(doc2);
    xmlFreeDoc(doc3);
    xmlFreeDoc(doc4);
    xmlFreeDoc(doc5);
    xmlCleanupParser();
    printf("done\n");
    return 0;
}
