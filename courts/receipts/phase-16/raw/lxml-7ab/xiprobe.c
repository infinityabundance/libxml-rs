#include <stdio.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xinclude.h>
static void dump(xmlNodePtr n, int d) {
  for (; n; n = n->next) {
    if (n->type == XML_ELEMENT_NODE) {
      int i;
      for (i=0;i<d;i++) printf(" ");
      printf("<%s", n->name);
      if (n->ns != NULL) {
        printf(" ns=%s", n->ns->prefix ? (char*)n->ns->prefix : "(default)");
        printf(" href=%s", n->ns->href ? (char*)n->ns->href : "(null)");
      }
      printf(">\n");
      dump(n->children, d+1);
    } else if (n->type == XML_TEXT_NODE && n->content && n->content[0] && n->content[0]!='\n') {
      int i;
      for (i=0;i<d;i++) printf(" ");
      printf("text:%s\n", n->content);
    }
  }
}
int main(int argc, char **argv) {
  xmlDocPtr d = xmlReadFile(argv[1], NULL, XML_PARSE_NOENT);
  if (!d) { printf("parse failed\n"); return 1; }
  int r = xmlXIncludeProcess(d);
  printf("xinclude_rc=%d\n", r);
  dump(d->children, 0);
  xmlFreeDoc(d);
  return 0;
}
