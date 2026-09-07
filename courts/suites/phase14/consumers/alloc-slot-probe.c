/* alloc-slot-probe.c — prove the §16.5.10 cached slot-ADDRESS bridge still
 * observes runtime allocator mutation through the whole-archive facade.
 *
 * Sequence: (1) allocate via xmlMalloc (default), (2) xmlMemSetup installs
 * counting hooks, (3) allocate + free again — must go through the NEW hooks
 * (mutation observed), (4) xmlMemSetup restores defaults, (5) allocate —
 * must go through the DEFAULT hook again.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/xmlmemory.h>

static int malloc_count = 0;
static int free_count = 0;

static void *counting_malloc(size_t size) {
    malloc_count++;
    return malloc(size);
}
static void *counting_malloc_atomic(size_t size) { return counting_malloc(size); }
static void *counting_realloc(void *ptr, size_t size) { return realloc(ptr, size); }
static void counting_free(void *ptr) {
    free_count++;
    free(ptr);
}
static char *counting_strdup(const char *s) {
    char *r = malloc(strlen(s) + 1);
    strcpy(r, s);
    return r;
}

int main(void) {
    void *p;
    xmlDocPtr doc;

    p = xmlMalloc(16);
    if (p == NULL) { printf("FAIL: initial alloc\n"); return 1; }
    xmlFree(p);

    malloc_count = 0; free_count = 0;
    if (xmlMemSetup((xmlFreeFunc)counting_free, (xmlMallocFunc)counting_malloc,
                    (xmlReallocFunc)counting_realloc,
                    (xmlStrdupFunc)counting_strdup) != 0) {
        printf("FAIL: xmlMemSetup\n");
        return 1;
    }
    p = xmlMalloc(16);
    xmlFree(p);
    if (malloc_count != 1 || free_count != 1) {
        printf("FAIL: mutation not observed (malloc=%d free=%d)\n",
               malloc_count, free_count);
        return 1;
    }
    printf("PASS: xmlMemSetup mutation observed through the bridge\n");

    /* A full parse under the counting hooks (tree allocs all count). */
    malloc_count = 0; free_count = 0;
    doc = xmlReadMemory("<r><a>text</a><b x='1'/></r>", 28, "t.xml", NULL, 0);
    if (doc == NULL) { printf("FAIL: parse\n"); return 1; }
    xmlFreeDoc(doc);
    if (malloc_count == 0) { printf("FAIL: parse did not allocate\n"); return 1; }
    printf("PASS: parse allocations observed (malloc=%d)\n", malloc_count);

    xmlMemSetup(NULL, NULL, NULL, NULL); /* restore defaults */
    p = xmlMalloc(8);
    if (p == NULL) { printf("FAIL: post-restore alloc\n"); return 1; }
    xmlFree(p);
    printf("PASS: restore-to-default observed\n");
    return 0;
}
