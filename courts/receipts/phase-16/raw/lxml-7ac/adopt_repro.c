#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/HTMLparser.h>
#include <libxml/tree.h>
#include <libxml/dict.h>

static xmlNodePtr first_child_elem(xmlNodePtr n) {
  xmlNodePtr c = n ? n->children : NULL;
  while (c) { if (c->type == XML_ELEMENT_NODE) return c; c = c->next; }
  return NULL;
}

int main(int argc, char **argv) {
  (void)argc; (void)argv;
  xmlDocPtr d1 = xmlReadMemory("<a xmlns=\"some:ns\"><b/></a>",
                               (int)strlen("<a xmlns=\"some:ns\"><b/></a>"), "t.xml", NULL, 0);
  xmlDocPtr d2 = htmlReadMemory("<p>foo</p>", (int)strlen("<p>foo</p>"), "t.html", NULL, 0);
  printf("d1=%p dict1=%p  d2=%p dict2=%p\n", (void*)d1, (void*)d1->dict, (void*)d2, (void*)d2->dict);

  xmlNodePtr root = xmlDocGetRootElement(d1);
  /* modern DOM clone: shallow xmlDocCopyNode, then children */
  xmlNodePtr clone = xmlDocCopyNode(root, d1, 0);
  printf("clone=%p name=%s own1=%d\n", (void*)clone, (char*)clone->name,
         d1->dict ? xmlDictOwns(d1->dict, clone->name) : -1);

  xmlNodePtr p = first_child_elem(first_child_elem(xmlDocGetRootElement(d2)));
  printf("p=%p\n", (void*)p);

  /* modern DOM adopt: unlink + xmlSetTreeDoc */
  xmlUnlinkNode(clone);
  xmlSetTreeDoc(clone, d2);
  printf("after setTreeDoc: clone->doc=%p name=%s name_own1=%d name_own2=%d\n",
         (void*)clone->doc, (char*)clone->name,
         d1->dict ? xmlDictOwns(d1->dict, clone->name) : -1,
         d2->dict ? xmlDictOwns(d2->dict, clone->name) : -1);
  xmlAddChild(p, clone);

  printf("free d2\n"); fflush(stdout);
  xmlFreeDoc(d2);
  printf("free d1\n"); fflush(stdout);
  xmlFreeDoc(d1);
  printf("all freed\n");
  return 0;
}
