/* .dov pLang runtime v0.3.
 * Agents: one global FIFO mailbox queue, run-to-completion, one thread.
 * Same program + same input = same message order (deterministic).
 * Heap data (string bytes, slice items) is immutable and never freed in v0.3. */
#include "dov_rt.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct dov_msg {
    void *agent;
    dov_handler_fn fn;
    void *args;
    struct dov_msg *next;
} dov_msg;

static dov_msg *q_head = NULL;
static dov_msg *q_tail = NULL;

_Noreturn void dov_panic(const char *msg, int32_t line) {
    fflush(stdout);
    fprintf(stderr, "dov panic: %s (line %d)\n", msg, (int)line);
    exit(101);
}

_Noreturn void dov_panic_str(dov_str msg, int32_t line) {
    fflush(stdout);
    fputs("dov panic: ", stderr);
    fwrite(msg.p, 1, (size_t)msg.len, stderr);
    fprintf(stderr, " (line %d)\n", (int)line);
    exit(101);
}

void *dov_alloc(size_t size) {
    void *p = calloc(1, size ? size : 1);
    if (!p) dov_panic("out of memory", 0);
    return p;
}

void dov_send(void *agent, dov_handler_fn fn, void *args) {
    dov_msg *m = dov_alloc(sizeof *m);
    m->agent = agent;
    m->fn = fn;
    m->args = args;
    if (q_tail) q_tail->next = m; else q_head = m;
    q_tail = m;
}

void dov_run(void) {
    while (q_head) {
        dov_msg *m = q_head;
        q_head = m->next;
        if (!q_head) q_tail = NULL;
        m->fn(m->agent, m->args);
        free(m->args);
        free(m);
    }
}

int64_t dov_add(int64_t a, int64_t b, int32_t line) {
    int64_t r;
    if (__builtin_add_overflow(a, b, &r)) dov_panic("integer overflow in `+`", line);
    return r;
}
int64_t dov_sub(int64_t a, int64_t b, int32_t line) {
    int64_t r;
    if (__builtin_sub_overflow(a, b, &r)) dov_panic("integer overflow in `-`", line);
    return r;
}
int64_t dov_mul(int64_t a, int64_t b, int32_t line) {
    int64_t r;
    if (__builtin_mul_overflow(a, b, &r)) dov_panic("integer overflow in `*`", line);
    return r;
}
int64_t dov_div(int64_t a, int64_t b, int32_t line) {
    if (b == 0) dov_panic("division by zero", line);
    if (a == INT64_MIN && b == -1) dov_panic("integer overflow in `/`", line);
    return a / b;
}
int64_t dov_rem(int64_t a, int64_t b, int32_t line) {
    if (b == 0) dov_panic("remainder by zero", line);
    if (a == INT64_MIN && b == -1) return 0;
    return a % b;
}
int64_t dov_neg(int64_t a, int32_t line) {
    if (a == INT64_MIN) dov_panic("integer overflow in unary `-`", line);
    return -a;
}

/* ---------- strings ---------- */

static dov_str str_new(const char *a, int64_t na, const char *b, int64_t nb) {
    char *p = dov_alloc((size_t)(na + nb + 1));
    if (na) memcpy(p, a, (size_t)na);
    if (nb) memcpy(p + na, b, (size_t)nb);
    p[na + nb] = '\0';
    return (dov_str){p, na + nb};
}

bool dov_str_eq(dov_str a, dov_str b) {
    return a.len == b.len && (a.len == 0 || memcmp(a.p, b.p, (size_t)a.len) == 0);
}

int64_t dov_str_at(dov_str s, int64_t i, int32_t line) {
    if (i < 0 || i >= s.len) dov_panic("string index out of bounds", line);
    return (int64_t)(unsigned char)s.p[i];
}

dov_str dov_str_range(dov_str s, int64_t a, int64_t b, int32_t line) {
    if (a < 0 || a > b || b > s.len) dov_panic("string range out of bounds", line);
    if (b == s.len) return (dov_str){s.p + a, b - a}; /* the suffix is still NUL-terminated */
    return str_new(s.p + a, b - a, "", 0);
}

dov_str dov_str_concat(dov_str a, dov_str b) {
    if (a.len == 0) return b;
    if (b.len == 0) return a;
    if (a.len > INT64_MAX - b.len - 1) dov_panic("string too long", 0);
    return str_new(a.p, a.len, b.p, b.len);
}

dov_str dov_i64_to_str(int64_t v) {
    char buf[32];
    int n = snprintf(buf, sizeof buf, "%lld", (long long)v);
    return str_new(buf, n, "", 0);
}

dov_str dov_bool_to_str(bool v) { return v ? DOV_S("true", 4) : DOV_S("false", 5); }

dov_str dov_char_of(int64_t c, int32_t line) {
    if (c < 0 || c > 255) dov_panic("`char_of` needs a byte value 0..255", line);
    char b = (char)(unsigned char)c;
    return str_new(&b, 1, "", 0);
}

dov_str dov_str_from_c(const char *p) {
    if (!p) return DOV_S("", 0);
    return (dov_str){p, (int64_t)strlen(p)};
}

/* ---------- slices ---------- */

struct dov_buf {
    int64_t used; /* items written; views never reach past this */
    int64_t cap;
    max_align_t data[]; /* items follow, suitably aligned */
};

static dov_buf *buf_new(int64_t cap, size_t size) {
    if (cap < 4) cap = 4;
    if ((uint64_t)cap > (uint64_t)(SIZE_MAX / 2) / (size ? size : 1)) dov_panic("slice too long", 0);
    dov_buf *b = dov_alloc(sizeof(dov_buf) + (size_t)cap * size);
    b->cap = cap;
    return b;
}

static char *items(dov_slice s, size_t size) {
    return (char *)s.buf->data + (size_t)s.off * size;
}

dov_slice dov_slice_lit(int64_t n, size_t size, const void *src) {
    dov_buf *b = buf_new(n, size);
    memcpy(b->data, src, (size_t)n * size);
    b->used = n;
    return (dov_slice){b, 0, n};
}

const void *dov_index(dov_slice s, int64_t i, size_t size, int32_t line) {
    if (i < 0 || i >= s.len) dov_panic("slice index out of bounds", line);
    return items(s, size) + (size_t)i * size;
}

dov_slice dov_range(dov_slice s, int64_t a, int64_t b, int32_t line) {
    if (a < 0 || a > b || b > s.len) dov_panic("slice range out of bounds", line);
    if (a == b) return dov_slice_empty();
    return (dov_slice){s.buf, s.off + a, b - a};
}

dov_slice dov_append(dov_slice s, size_t size, const void *item) {
    /* In place when this view ends where the buffer's written items end:
     * no other view can see the new slot, so nothing visible changes. */
    if (s.buf && s.off + s.len == s.buf->used && s.buf->used < s.buf->cap) {
        memcpy((char *)s.buf->data + (size_t)s.buf->used * size, item, size);
        s.buf->used++;
        return (dov_slice){s.buf, s.off, s.len + 1};
    }
    dov_buf *b = buf_new(s.len * 2, size);
    if (s.len) memcpy(b->data, items(s, size), (size_t)s.len * size);
    memcpy((char *)b->data + (size_t)s.len * size, item, size);
    b->used = s.len + 1;
    return (dov_slice){b, 0, s.len + 1};
}

dov_slice dov_concat(dov_slice a, dov_slice b, size_t size) {
    if (b.len == 0) return a;
    if (a.len == 0) return b;
    dov_buf *n = buf_new(a.len + b.len, size);
    memcpy(n->data, items(a, size), (size_t)a.len * size);
    memcpy((char *)n->data + (size_t)a.len * size, items(b, size), (size_t)b.len * size);
    n->used = a.len + b.len;
    return (dov_slice){n, 0, a.len + b.len};
}

/* ---------- output ---------- */

void dov_print_i64(int64_t v) { printf("%lld", (long long)v); }
void dov_print_bool(bool v) { fputs(v ? "true" : "false", stdout); }
void dov_print_str(dov_str v) { fwrite(v.p, 1, (size_t)v.len, stdout); }
void dov_print_nl(void) { putchar('\n'); }
