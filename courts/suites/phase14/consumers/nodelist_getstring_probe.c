/* Differential court probe: xmlNodeListGetString over a non-text node list.
 *
 * SimpleXML __toString (trim($xml)) reaches xmlNodeListGetString with an
 * ELEMENT node list head that yields no text; the walker must return a
 * NUL-terminated empty string. The pre-fix xml_strdup(b"") idiom handed the
 * duplicate a dangling 0x1 pointer (Rust empty byte-string literal), which
 * crashed in xml_strlen. Both sides must print identical lines and exit 0.
 */
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <stdio.h>
#include <string.h>
int main(void) {
    const char *x = "<people></people>";
    xmlDocPtr d = xmlReadMemory(x, (int) strlen(x), NULL, NULL, XML_PARSE_NONET);
    if (!d) return 2;
    xmlNodePtr r = xmlDocGetRootElement(d);
    xmlNodePtr person = xmlNewTextChild(r, NULL, BAD_CAST "person", BAD_CAST "Joe");
    xmlSetProp(person, BAD_CAST "gender", BAD_CAST "male");
    /* element list head -> empty string; text child list -> "Joe" */
    xmlChar *s1 = xmlNodeListGetString(d, r->children, 1);
    xmlChar *s2 = xmlNodeListGetString(d, person->children, 1);
    printf("ELEM=[%s] len=%lu\n", s1 ? (char *) s1 : "(null)",
           (unsigned long) (s1 ? strlen((char *) s1) : 0));
    printf("TEXT=[%s] len=%lu\n", s2 ? (char *) s2 : "(null)",
           (unsigned long) (s2 ? strlen((char *) s2) : 0));
    xmlFree(s1);
    xmlFree(s2);
    xmlFreeDoc(d);
    return 0;
}
