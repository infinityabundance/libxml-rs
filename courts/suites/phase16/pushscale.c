/* pushscale.c — time xmlCreatePushParserCtxt + xmlParseChunk over N chunks
 * of newline-heavy content (the lxml test_very_large_sourceline_iterparse
 * shape, scaled). Usage: pushscale <nchunks> <chunk_kb> */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <libxml/parser.h>

int main(int argc, char **argv) {
    int n = atoi(argv[1]);
    int kb = atoi(argv[2]);
    size_t chunk = (size_t)kb * 1024;
    char *buf = malloc(chunk);
    memset(buf, '\n', chunk);

    xmlParserCtxtPtr ctxt = xmlCreatePushParserCtxt(NULL, NULL, NULL, 0, NULL);
    if (!ctxt) return 1;
    const char *prolog = "<?xml version=\"1.0\"?>\n<root>\n";
    xmlParseChunk(ctxt, prolog, strlen(prolog), 0);
    double t0 = (double)clock() / CLOCKS_PER_SEC;
    long total = 0;
    for (int i = 0; i < n; i++) {
        /* chunk = newlines + a tag, like the lxml test */
        xmlParseChunk(ctxt, buf, chunk, 0);
        xmlParseChunk(ctxt, "<br/>", 5, 0);
        total += chunk + 5;
    }
    xmlParseChunk(ctxt, "</root>", 7, 1);
    double t1 = (double)clock() / CLOCKS_PER_SEC;
    printf("chunks=%d bytes=%ld secs=%.2f MB/s=%.1f wellFormed=%d\n",
           n, total, t1 - t0, (double)total / (1 << 20) / (t1 - t0),
           ctxt->wellFormed);
    xmlFreeParserCtxt(ctxt);
    free(buf);
    return 0;
}
