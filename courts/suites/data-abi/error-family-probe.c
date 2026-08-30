/*
 * ERROR-001 — error semantics differential probe (11.1-M).
 *
 * Compiles against the system libxml2 (oracle) and the candidate DSO and
 * must produce byte-identical stdout.
 *
 * For every deterministic malformed/edge input the probe runs four passes:
 *
 *   1. default  — no handlers installed; the library's default generic
 *                 handler writes to stderr, which the probe captures by
 *                 redirecting fd 2 to a temp file and replaying it (escaped)
 *                 into stdout so ordering is deterministic;
 *   2. struct   — a structured handler prints every xmlError field
 *                 (domain/code/level/line/int1/int2/file/str1/str2/str3/msg);
 *   3. frag     — a generic handler prints every xmlFormatError fragment
 *                 exactly as the library formats it (vsnprintf in the probe);
 *   4. noerr    — same as default but with XML_PARSE_NOERROR|NOWARNING to
 *                 prove suppression is honored (expect empty stderr).
 *
 * After each pass the global last error (xmlGetLastError) and the parse
 * result (doc non-null) are printed.
 *
 * The source-window/caret and 80-column window cap are exercised by long
 * single-line inputs; UTF-8 continuation bytes, CRLF, and multi-line inputs
 * exercises xmlParserInputGetWindow parity.
 */

#define _POSIX_C_SOURCE 200809L

#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <fcntl.h>
#include <unistd.h>

/* ── escaping helpers ───────────────────────────────────────────────────── */

static void esc_print(const char *s, int len)
{
    int i;
    for (i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
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

static void print_str(const char *s)
{
    if (s == NULL) {
        fputs("(null)", stdout);
    } else {
        esc_print(s, (int)strlen(s));
    }
}

/* ── handlers ────────────────────────────────────────────────────────────── */

static void struct_handler(void *ctx, const xmlError *err)
{
    int *count = (int *)ctx;
    (*count)++;
    printf("struct %d: domain=%d code=%d level=%d line=%d int1=%d int2=%d "
           "file=", *count, err->domain, err->code, err->level, err->line,
           err->int1, err->int2);
    print_str(err->file);
    fputs(" str1=", stdout);
    print_str(err->str1);
    fputs(" str2=", stdout);
    print_str(err->str2);
    fputs(" str3=", stdout);
    print_str(err->str3);
    fputs(" msg=", stdout);
    print_str(err->message);
    fputs("\n", stdout);
}

static void generic_handler(void *ctx, const char *msg, ...)
{
    char buf[1024];
    va_list ap;
    (void)ctx;
    va_start(ap, msg);
    vsnprintf(buf, sizeof(buf), msg, ap);
    va_end(ap);
    fputs("frag: ", stdout);
    esc_print(buf, (int)strlen(buf));
    fputs("\n", stdout);
}

static void print_last_error(const char *tag)
{
    const xmlError *e = xmlGetLastError();
    if (e == NULL || e->code == XML_ERR_OK) {
        printf("%s: (none)\n", tag);
        return;
    }
    printf("%s: domain=%d code=%d level=%d line=%d int1=%d int2=%d file=",
           tag, e->domain, e->code, e->level, e->line, e->int1, e->int2);
    print_str(e->file);
    fputs(" str1=", stdout);
    print_str(e->str1);
    fputs(" str2=", stdout);
    print_str(e->str2);
    fputs(" str3=", stdout);
    print_str(e->str3);
    fputs(" msg=", stdout);
    print_str(e->message);
    fputs("\n", stdout);
}

/* ── case table ──────────────────────────────────────────────────────────── */

struct Case {
    const char *input;
    const char *url;
};

static const struct Case cases[] = {
    /* empty / no root */
    { "", NULL },
    { "   \n  ", NULL },
    { "text", NULL },
    { "<!DOCTYPE a>", NULL },
    /* element / tag errors */
    { "<a>", NULL },
    { "<a><b>", NULL },
    { "<a></b>", NULL },
    { "<a></a>x", NULL },
    { "</a>", NULL },
    { "<a/><a/>", NULL },
    { "<1a/>", NULL },
    { "<a", NULL },
    /* references */
    { "<a>&b", NULL },
    { "<a>&b</a>", NULL },
    { "<a>&", NULL },
    { "<a>&#x41", NULL },
    { "<a>&#xZZ;</a>", NULL },
    { "<a>&#x110000;</a>", NULL },
    { "<a>&#0;</a>", NULL },
    { "<a>&b;</a>", NULL },
    /* attributes */
    { "<a b=c/>", NULL },
    { "<a b/>", NULL },
    { "<a b=\"1\" b=\"2\"/>", NULL },
    { "<a b='c", NULL },
    { "<a b=\"&c/>", NULL },
    { "<a b=\"\xC3\x28\"/>", NULL },
    /* content */
    { "<a>]]></a>", NULL },
    { "<a>\x01</a>", NULL },
    { "<a>\xC3\x28</a>", NULL },
    { "<a>\xEF\xBF\xBF</a>", NULL },
    /* markup */
    { "<?>", NULL },
    { "<?xml version=\"1.0\"", NULL },
    { "<!--", NULL },
    { "<![CDATA[foo", NULL },
    { "<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>", NULL },
    /* multiline / window / caret */
    { "<a>\n  &b", NULL },
    { "<a>\r\n  &b", NULL },
    { "<a>\xC3\xA9&b</a>", NULL },
    /* long line: 80-column window cap */
    { "<a>xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx&b</a>", NULL },
    { "<a>xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\x01</a>", NULL },
    /* filename variants */
    { "<a>", "e.xml" },
    { "<a>&b</a>", "e.xml" },
    { "<a></b>", "e.xml" },
    { "<a b=c/>", "e.xml" },
    { "text", "e.xml" },
    { "<!--", "e.xml" },
    { "<a>\x01</a>", "e.xml" },
    { "<a>]]></a>", "e.xml" },
};

#define NCASES ((int)(sizeof(cases) / sizeof(cases[0])))

/* ── passes ──────────────────────────────────────────────────────────────── */

static void reset_handlers(void)
{
    xmlSetStructuredErrorFunc(NULL, NULL);
    xmlSetGenericErrorFunc(NULL, NULL);
}

/* Pass 1: default handlers; capture stderr by redirecting fd 2. */
static void pass_default(const struct Case *c)
{
    char tmpl[] = "/tmp/errprobeXXXXXX";
    char rbuf[8192];
    int fd, saved, n;
    xmlDocPtr doc;

    reset_handlers();
    xmlResetLastError();
    fd = mkstemp(tmpl);
    saved = dup(2);
    dup2(fd, 2);
    doc = xmlReadMemory(c->input, (int)strlen(c->input), c->url, NULL, 0);
    fflush(stderr);
    dup2(saved, 2);
    close(saved);
    lseek(fd, 0, SEEK_SET);
    fputs("  default: ", stdout);
    while ((n = (int)read(fd, rbuf, sizeof(rbuf))) > 0)
        esc_print(rbuf, n);
    close(fd);
    unlink(tmpl);
    fputs("\n", stdout);
    print_last_error("  last");
    printf("  doc=%d\n", doc != NULL);
    if (doc != NULL)
        xmlFreeDoc(doc);
}

/* Pass 2: structured handler. */
static void pass_struct(const struct Case *c)
{
    int count = 0;
    xmlDocPtr doc;

    reset_handlers();
    xmlSetStructuredErrorFunc(&count, struct_handler);
    xmlResetLastError();
    doc = xmlReadMemory(c->input, (int)strlen(c->input), c->url, NULL, 0);
    printf("  struct-count=%d\n", count);
    print_last_error("  last");
    printf("  doc=%d\n", doc != NULL);
    if (doc != NULL)
        xmlFreeDoc(doc);
}

/* Pass 3: generic fragment handler. */
static void pass_frag(const struct Case *c)
{
    int count = 0;
    xmlDocPtr doc;

    reset_handlers();
    xmlSetGenericErrorFunc(&count, generic_handler);
    xmlResetLastError();
    doc = xmlReadMemory(c->input, (int)strlen(c->input), c->url, NULL, 0);
    printf("  frag-count=%d\n", count);
    print_last_error("  last");
    printf("  doc=%d\n", doc != NULL);
    if (doc != NULL)
        xmlFreeDoc(doc);
}

/* Pass 4: suppression flags with default handlers. */
static void pass_noerr(const struct Case *c)
{
    char tmpl[] = "/tmp/errprobeXXXXXX";
    char rbuf[8192];
    int fd, saved, n;
    xmlDocPtr doc;

    reset_handlers();
    xmlResetLastError();
    fd = mkstemp(tmpl);
    saved = dup(2);
    dup2(fd, 2);
    doc = xmlReadMemory(c->input, (int)strlen(c->input), c->url, NULL,
                        XML_PARSE_NOERROR | XML_PARSE_NOWARNING);
    fflush(stderr);
    dup2(saved, 2);
    close(saved);
    lseek(fd, 0, SEEK_SET);
    fputs("  noerr: ", stdout);
    while ((n = (int)read(fd, rbuf, sizeof(rbuf))) > 0)
        esc_print(rbuf, n);
    close(fd);
    unlink(tmpl);
    fputs("\n", stdout);
    printf("  doc=%d\n", doc != NULL);
    if (doc != NULL)
        xmlFreeDoc(doc);
}

int main(void)
{
    int i;

    setvbuf(stdout, NULL, _IONBF, 0);
    for (i = 0; i < NCASES; i++) {
        const struct Case *c = &cases[i];
        printf("=== %d: url=%s input=", i, c->url ? c->url : "(null)");
        esc_print(c->input, (int)strlen(c->input));
        fputs(" ===\n", stdout);
        pass_default(c);
        pass_struct(c);
        pass_frag(c);
        pass_noerr(c);
    }
    return 0;
}
