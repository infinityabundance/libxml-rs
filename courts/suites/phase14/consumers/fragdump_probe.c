/* Differential court probe: xmlNodeDumpOutput semantics on a document
 * fragment must emit its children (upstream xmlsave.c 2.15 handles
 * XML_DOCUMENT_FRAG_NODE by trampolining into the children). Regression for
 * Phase 14.3 PHP DOMParentNode_empty_argument (candidate previously dumped an
 * empty fragment => empty saveXML + PHP double-destroy).
 *
 * Build+run identical against oracle libxml (pkg-config libxml-2.0) and the
 * candidate. Both must print identical bytes.
 */
#include <libxml/tree.h>
#include <libxml/xmlsave.h>
#include <libxml/xmlIO.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { char *buf; size_t len, cap; } sink;
static int sink_w(void *ctx, const char *b, int n){ sink*s=ctx; if((s->len+n+1)>s->cap){s->cap=(s->len+n)*2+16;s->buf=realloc(s->buf,s->cap);}memcpy(s->buf+s->len,b,n);s->len+=n;s->buf[s->len]=0;return n;}
static int sink_c(void *ctx){ (void)ctx; return 0; }

int main(void) {
    xmlDocPtr doc = xmlNewDoc(BAD_CAST "1.0");
    xmlNodePtr root = xmlNewDocNode(doc, NULL, BAD_CAST "root", NULL);
    xmlDocSetRootElement(doc, root);

    xmlNodePtr frag = xmlNewDocFragment(doc);
    xmlNodePtr foo = xmlNewDocNode(doc, NULL, BAD_CAST "foo", NULL);
    xmlAddChild(frag, foo);

    /* Fragment dump (should emit <foo/>). */
    sink s = {0};
    xmlOutputBufferPtr out = xmlOutputBufferCreateIO(sink_w, sink_c, &s, NULL);
    xmlNodeDumpOutput(out, doc, frag, 0, 0, NULL);
    xmlOutputBufferFlush(out);
    xmlOutputBufferClose(out);
    printf("FRAG=[%s] len=%zu\n", s.buf ? s.buf : "(null)", s.len);
    free(s.buf);

    /* Element-alone control (must remain <foo/>). */
    memset(&s, 0, sizeof s);
    out = xmlOutputBufferCreateIO(sink_w, sink_c, &s, NULL);
    xmlNodeDumpOutput(out, doc, foo, 0, 0, NULL);
    xmlOutputBufferFlush(out);
    xmlOutputBufferClose(out);
    printf("ELM=[%s] len=%zu\n", s.buf ? s.buf : "(null)", s.len);
    free(s.buf);

    xmlFreeDoc(doc);
    return 0;
}
