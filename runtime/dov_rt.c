/* .dov pLang runtime v0.1.
 * Agents: one global FIFO mailbox queue, run-to-completion, one thread.
 * Same program + same input = same message order (deterministic). */
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

void dov_panic(const char *msg, int32_t line) {
    fflush(stdout);
    fprintf(stderr, "dov panic: %s (line %d)\n", msg, (int)line);
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
bool dov_str_eq(const char *a, const char *b) { return strcmp(a, b) == 0; }

void dov_print_i64(int64_t v) { printf("%lld", (long long)v); }
void dov_print_bool(bool v) { fputs(v ? "true" : "false", stdout); }
void dov_print_str(const char *v) { fputs(v, stdout); }
void dov_print_nl(void) { putchar('\n'); }
