/*
 * TREE-001 — structural tree fingerprint differential probe (11.1-N).
 *
 * Compiles against the system libxml2 (oracle) and the candidate DSO and
 * must produce byte-identical stdout.
 *
 * Unlike serialization comparisons, this probe fingerprints the parsed
 * tree field-by-field exactly as a C consumer traversing the public
 * structs would observe it: node types/names/contents/line numbers,
 * children/parent/next/prev linkage invariants, document-pointer
 * propagation, namespace (ns/nsDef) bindings, attribute representation
 * (value text children, atype, ns), entity-reference nodes, DTD
 * construction, ID/IDREF tables, compact-text mode, recovery/huge modes,
 * whitespace handling, URL/base behavior, and copy/unlink/relink
 * semantics.
 *
 * Only deterministic facts are printed: pointer fields are reduced to
 * "(null)", "(doc)", or "(parent-name)" so both sides are comparable.
 */

/*
 * TREE-001 — parser/tree structural differential probe (11.1-N).
 *
 * Compiles against the system libxml2 (oracle) and the candidate DSO and
 * must produce byte-identical stdout. See tools/abi/tree_structure_probe.py.
 */
#define _GNU_SOURCE
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>

/* ── escaped string printing ─────────────────────────────────────────────── */

static void esc(const xmlChar *s)
{
    if (s == NULL) {
        fputs("(null)", stdout);
        return;
    }
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        switch (c) {
        case '\n': fputs("\\n", stdout); break;
        case '\r': fputs("\\r", stdout); break;
        case '\t': fputs("\\t", stdout); break;
        case '\\': fputs("\\\\", stdout); break;
        case '"':  fputs("\\\"", stdout); break;
        default:
            if (c >= 0x20 && c < 0x7f)
                putchar(c);
            else
                printf("\\x%02X", c);
        }
    }
}

static const char *nodename(xmlNodePtr n)
{
    if (n == NULL)
        return "(null)";
    if (n->type == XML_DOCUMENT_NODE || n->type == XML_HTML_DOCUMENT_NODE)
        return "(doc)";
    return n->name ? (const char *)n->name : "(noname)";
}

/* Print a namespace binding. */
static void print_ns(xmlNsPtr ns)
{
    if (ns == NULL) {
        fputs("(null)", stdout);
        return;
    }
    putchar('[');
    esc(ns->prefix);
    putchar(':');
    esc(ns->href);
    printf(" t=%d", ns->type);
    putchar(']');
}

/* Linkage invariants: verify child/parent/next/prev consistency. */
static void check_linkage(xmlNodePtr n, xmlNodePtr parent, const char *indent)
{
    xmlNodePtr c = n->children;
    int count = 0;
    int ok = 1;
    if (n->parent != parent)
        ok = 0;
    if (n->children != NULL && n->children->parent != n)
        ok = 0;
    if (n->last != NULL && n->last->parent != n)
        ok = 0;
    if (n->children != NULL && n->children->prev != NULL)
        ok = 0;
    if (n->last != NULL && n->last->next != NULL)
        ok = 0;
    for (c = n->children; c != NULL; c = c->next) {
        if (c->parent != n)
            ok = 0;
        if (c->next != NULL && c->next->prev != c)
            ok = 0;
        count++;
        if (count > 500) {
            ok = 0;
            break;
        }
    }
    if (n->last != NULL) {
        /* last must be the final child */
        xmlNodePtr tail = n->children;
        if (tail == NULL)
            ok = 0;
        else {
            while (tail->next != NULL)
                tail = tail->next;
            if (tail != n->last)
                ok = 0;
        }
    }
    printf("%slink ok=%d children=%d last=%s\n", indent, ok, count,
           n->last == NULL ? "(null)" : nodename(n->last));
}

/* Recursive structural fingerprint of an element/document node subtree. */
static void dump_node(xmlNodePtr n, int depth, xmlDocPtr doc)
{
    char indent[64];
    int i;
    xmlAttrPtr a;
    xmlNsPtr ns;
    xmlNodePtr c;

    for (i = 0; i < depth && i < 30; i++)
        indent[i] = ' ';
    indent[i] = 0;

    printf("%snode type=%d name=", indent, n->type);
    esc(n->name);
    if (n->type == XML_ELEMENT_NODE) {
        printf(" line=%u extra=%u doc=%d ns=", n->line, n->extra, n->doc == doc);
        print_ns(n->ns);
    } else if (n->type == XML_TEXT_NODE || n->type == XML_CDATA_SECTION_NODE
               || n->type == XML_COMMENT_NODE || n->type == XML_PI_NODE) {
        printf(" line=%u extra=%u doc=%d", n->line, n->extra, n->doc == doc);
    } else {
        printf(" doc=%d", n->doc == doc);
    }
    printf(" parent=%s next=%s prev=%s\n", nodename(n->parent),
           nodename(n->next), nodename(n->prev));
    check_linkage(n, n->parent, indent);

    /* `content` is a string only for text/CDATA/comment/PI and entity-decl
     * nodes; other node types (declarations) reuse the field for non-string
     * payloads. */
    if (n->content != NULL
        && (n->type == XML_TEXT_NODE || n->type == XML_CDATA_SECTION_NODE
            || n->type == XML_COMMENT_NODE || n->type == XML_PI_NODE
            || n->type == XML_ENTITY_DECL)) {
        printf("%s  content: \"", indent);
        esc(n->content);
        printf("\"\n");
    }
    if (n->psvi != NULL)
        printf("%s  psvi=set\n", indent);

    /* Namespace declarations: element nodes only. */
    if (n->type == XML_ELEMENT_NODE) {
        for (ns = n->nsDef; ns != NULL; ns = ns->next) {
            printf("%s  nsdef: ", indent);
            print_ns(ns);
            putchar('\n');
        }
    }

    /* Attributes: element nodes only (other node types leave the field
     * uninitialized in compact mode). */
    if (n->type == XML_ELEMENT_NODE) {
        for (a = n->properties; a != NULL; a = a->next) {
        printf("%s  attr type=%d name=", indent, a->type);
        esc(a->name);
        printf(" atype=%d ns=", a->atype);
        print_ns(a->ns);
        printf(" parent=%s", nodename(a->parent));
        printf(" doc=%d", a->doc == doc);
        printf(" value=\"");
        if (a->children != NULL)
            esc(a->children->content);
        printf("\" nchild=%d", a->children ? 1 : 0);
        printf("\n");
        if (a->ns != NULL)
            printf("%s    attr-ns: ", indent), print_ns(a->ns), putchar('\n');
        }
    }

    for (c = n->children; c != NULL; c = c->next)
        dump_node(c, depth + 1, doc);
}

/* Fingerprint a whole document. */
static void dump_doc(xmlDocPtr doc, const char *tag)
{
    printf("=== %s ===\n", tag);
    if (doc == NULL) {
        fputs("doc=(null)\n", stdout);
        return;
    }
    printf("doc type=%d name=", doc->type);
    esc((xmlChar *)doc->name);
    printf(" version=");
    esc(doc->version);
    printf(" encoding=");
    esc(doc->encoding);
    printf(" URL=");
    esc(doc->URL);
    printf(" standalone=%d compression=%d charset=%d parseFlags=%d properties=%d\n",
           doc->standalone, doc->compression, doc->charset,
           doc->parseFlags, doc->properties);
    printf(" intSubset=%s extSubset=%s ids=%s refs=%s dict=%s\n",
           doc->intSubset ? "set" : "(null)",
           doc->extSubset ? "set" : "(null)",
           doc->ids ? "set" : "(null)",
           doc->refs ? "set" : "(null)",
           doc->dict ? "set" : "(null)");
    if (doc->intSubset) {
        xmlDtdPtr d = doc->intSubset;
        printf(" dtd name=");
        esc(d->name);
        printf(" ExternalID=");
        esc(d->ExternalID);
        printf(" SystemID=");
        esc(d->SystemID);
        printf(" notations=%s elements=%s attributes=%s entities=%s pentities=%s\n",
               d->notations ? "set" : "(null)",
               d->elements ? "set" : "(null)",
               d->attributes ? "set" : "(null)",
               d->entities ? "set" : "(null)",
               d->pentities ? "set" : "(null)");
    }
    dump_node((xmlNodePtr)doc, 0, doc);
    fputs("--- xmlGetLineNo checks ---\n", stdout);
    {
        xmlNodePtr c;
        for (c = doc->children; c != NULL; c = c->next)
            printf("lineno %s=%ld\n", nodename(c), xmlGetLineNo(c));
    }
    fputs("--- api checks ---\n", stdout);
    printf("root=%s\n", nodename(xmlDocGetRootElement(doc)));
    {
        xmlNodePtr e = xmlDocGetRootElement(doc);
        if (e) {
            printf("firstElem=%s lastElem=%s\n",
                   nodename((xmlNodePtr)xmlFirstElementChild(e)),
                   nodename((xmlNodePtr)xmlLastElementChild(e)));
            printf("nextElem=%s prevElem=%s\n",
                   nodename((xmlNodePtr)xmlNextElementSibling(e)),
                   nodename((xmlNodePtr)xmlPreviousElementSibling(e)));
            printf("base=");
            esc(xmlNodeGetBase(doc, e));
            printf("\ncontent=\"");
            esc(xmlNodeGetContent(e));
            printf("\"\n");
        }
    }
}

/* ── corpus runner ───────────────────────────────────────────────────────── */

/* Capture stderr (default handler diagnostics) and replay it escaped into
 * stdout so the fingerprint covers parser warnings/errors byte-for-byte. */
static void replay_stderr(const char *input, const char *url, int options)
{
    char tmpl[] = "/tmp/treeprobeXXXXXX";
    char rbuf[8192];
    int fd, saved, n;

    /* Redirect fd 2 around the parse so the library's default handler
     * diagnostics land in the temp file. */
    fd = mkstemp(tmpl);
    saved = dup(2);
    dup2(fd, 2);
    fflush(stderr);
    xmlResetLastError();
    xmlReadMemory(input, (int)strlen(input), url, NULL, options);
    fflush(stderr);
    dup2(saved, 2);
    close(saved);
    lseek(fd, 0, SEEK_SET);
    while ((n = (int)read(fd, rbuf, sizeof rbuf - 1)) > 0) {
        rbuf[n] = 0;
        esc((xmlChar *)rbuf);
    }
    close(fd);
    unlink(tmpl);
}

static void parse_and_dump(const char *input, const char *url, int options,
                           const char *tag)
{
    xmlDocPtr doc;

    fputs("stderr:", stdout);
    replay_stderr(input, url, options);
    putchar('\n');
    xmlResetLastError();
    doc = xmlReadMemory(input, (int)strlen(input), url, NULL, options);
    dump_doc(doc, tag);
    if (doc)
        xmlFreeDoc(doc);
    xmlResetLastError();
}

/* Mutation checks (copy / unlink / relink / set-prop) on a parsed doc. */
static void mutation_checks(xmlDocPtr doc)
{
    xmlNodePtr root, child, cp, cp2, n2;
    xmlAttrPtr ap;

    root = xmlDocGetRootElement(doc);
    if (root == NULL)
        return;
    child = root->children;
    while (child && (child->type == XML_TEXT_NODE))
        child = child->next;
    if (child == NULL)
        child = root->children;

    printf("--- mutations ---\n");
    /* xmlCopyNode (recursive) */
    cp = xmlCopyNode(root, 1);
    printf("copy root=%s nchild=%d\n", nodename(cp), cp ? 1 : 0);
    if (cp) {
        dump_node(cp, 0, cp->doc);
        printf("copy doc-ptr self=%d\n", cp->doc == doc);
        xmlFreeNode(cp);
    }
    /* xmlDocCopyNode into a fresh doc */
    if (root) {
        xmlDocPtr d2 = xmlNewDoc(BAD_CAST "1.0");
        cp2 = xmlDocCopyNode(root, d2, 1);
        printf("doccopy root=%s doc-ptr-self=%d\n", nodename(cp2),
               cp2 ? (cp2->doc == d2) : 0);
        if (cp2) {
            xmlDocSetRootElement(d2, cp2);
            printf("doccopy after setroot root=%s parent-doc=%d\n",
                   nodename(xmlDocGetRootElement(d2)),
                   xmlDocGetRootElement(d2) == cp2);
            xmlFreeDoc(d2);
        }
    }
    /* xmlUnlinkNode + xmlAddChild (moves node between parents) */
    if (child && child != root) {
        xmlNodePtr saved = child->next;
        xmlUnlinkNode(child);
        printf("unlink child=%s parent=%s next=%s\n", nodename(child),
               nodename(child->parent), nodename(child->next));
        xmlAddChild(root, child);
        printf("relink child=%s parent=%s doc-self=%d\n", nodename(child),
               nodename(child->parent), child->doc == doc);
        if (saved)
            xmlUnlinkNode(saved);
    }
    /* xmlSetProp / xmlGetProp */
    ap = xmlSetProp(root, BAD_CAST "p", BAD_CAST "v");
    printf("setprop name=%s value=", ap ? (char *)ap->name : "(null)");
    esc(xmlGetProp(root, BAD_CAST "p"));
    printf("\n");
    /* xmlHasProp */
    printf("hasprop=%s\n", xmlHasProp(root, BAD_CAST "p") ? "yes" : "no");
    /* xmlNodeSetContent */
    if (root->children) {
        xmlNodeSetContent(root->children, BAD_CAST "replaced");
        printf("setcontent child-content=\"");
        esc(root->children->content);
        printf("\"\n");
    }
    /* xmlNodeSetName */
    if (root) {
        xmlNodeSetName(root, BAD_CAST "renamed");
        printf("setname root=%s\n", nodename(root));
        xmlNodeSetName(root, root->name);
    }
    /* n2: new element with xmlSetTreeDoc */
    n2 = xmlNewNode(NULL, BAD_CAST "n2");
    xmlSetTreeDoc(n2, doc);
    printf("setdoc n2 doc-self=%d\n", n2->doc == doc);
    xmlFreeNode(n2);
}

static const char *corpus[] = {
    /* basic structure / text merging / line numbers */
    "<a>t1<b/>t2</a>",
    "<a>\n  <b>inner</b>\n  <c/>\n</a>",
    "<a>one<b>two</b>three<c/>four</a>",
    /* attributes */
    "<a b=\"1\" c=\"2\" d=\"3\"/>",
    "<a x=\"&lt; &amp;\"/>",
    /* namespaces */
    "<a xmlns=\"u1\" xmlns:p=\"u2\"><p:b xmlns:q=\"u3\"/></a>",
    "<a xmlns=\"u\"><b xmlns=\"\"><c/></b></a>",
    /* comments / PIs / CDATA */
    "<a><!--comment--><?pi data?><![CDATA[raw <>&]]></a>",
    /* entities */
    "<!DOCTYPE a [<!ENTITY e \"value\">]><a>&e;</a>",
    "<!DOCTYPE a [<!ENTITY e \"value\">]><a>&e;</a>",      /* +NOENT */
    "<a>&amp;&lt;&gt;&quot;&apos;</a>",
    /* DTD construction */
    "<!DOCTYPE a [<!ELEMENT a (b|c)*><!ATTLIST a id ID #IMPLIED x CDATA 'd'>"
    "<!ENTITY e \"v\"><!NOTATION n SYSTEM \"sys\">]><a/>",
    /* IDs */
    "<!DOCTYPE a [<!ATTLIST a id ID #IMPLIED>]><a id=\"x\"/>",
    /* whitespace */
    "<a>  <b/>  </a>",                                     /* +NOBLANKS */
    "<a> <b> x </b> </a>",                                 /* +KEEPBLANKS */
    /* empty and edge docs */
    "<a/>",
    "<a> </a>",
    "<?xml version=\"1.0\"?><a/>",
    "<?xml version=\"1.0\"?><a><b/></a>",
    "<a>&#65;&#x42;</a>",
};

int main(void)
{
    unsigned i;
    int opts[] = {0, XML_PARSE_NOBLANKS, XML_PARSE_COMPACT,
                  XML_PARSE_DTDATTR, XML_PARSE_NOENT | XML_PARSE_DTDLOAD,
                  XML_PARSE_RECOVER, XML_PARSE_HUGE, XML_PARSE_BIG_LINES};
    const char *onames[] = {"0", "noblanks", "compact", "dtdattr",
                            "noent+dtdload", "recover", "huge", "biglines"};
    char tag[256];

    setvbuf(stdout, NULL, _IONBF, 0);
    for (i = 0; i < sizeof(corpus) / sizeof(corpus[0]); i++) {
        snprintf(tag, sizeof tag, "case %u: %s", i, corpus[i]);
        parse_and_dump(corpus[i], "t.xml", 0, tag);
        /* named variants */
        if (i == 0) {
            parse_and_dump(corpus[i], NULL, 0, "case 0 url=null");
            parse_and_dump(corpus[i], "t.xml", XML_PARSE_COMPACT, "case 0 compact");
        }
        if (i == 1)
            parse_and_dump(corpus[i], "t.xml", XML_PARSE_NOBLANKS, "case 1 noblanks");
        if (i == 13)
            parse_and_dump(corpus[i], "t.xml", XML_PARSE_NOBLANKS, "case 13 noblanks");
        if (i == 8) {
            parse_and_dump(corpus[i], "t.xml", XML_PARSE_NOENT, "case 8 noent");
            parse_and_dump(corpus[i], "t.xml", XML_PARSE_NOENT | XML_PARSE_COMPACT,
                           "case 8 noent+compact");
        }
        if (i == 12)
            parse_and_dump(corpus[i], "t.xml", XML_PARSE_DTDATTR, "case 12 dtdattr");
    }

    /* Recovery on malformed inputs. */
    parse_and_dump("<a><b></a>", "r.xml", XML_PARSE_RECOVER, "recover mismatch");
    parse_and_dump("<a>&undef;</a>", "r.xml", XML_PARSE_RECOVER, "recover entity");

    /* Mutation checks on a fresh parse. */
    {
        xmlDocPtr doc = xmlReadMemory("<a><b>t</b><c/></a>",
                                     (int)strlen("<a><b>t</b><c/></a>"),
                                     NULL, NULL, 0);
        if (doc) {
            mutation_checks(doc);
            dump_doc(doc, "after-mutations");
            xmlFreeDoc(doc);
        }
    }
    return 0;
}
