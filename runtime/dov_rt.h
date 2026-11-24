/* .dov pLang runtime v0.3 — agent mailboxes, checked arithmetic, strings and slices.
 * Generated code includes only this header. It declares no libc
 * functions, so `extern "C"` declarations in .dov code cannot clash. */
#ifndef DOV_RT_H
#define DOV_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

_Static_assert(sizeof(long) == 8, "dov targets LP64 (64-bit long)");

/* Generated static functions may be unused (an unsent handler, an uncalled fn);
 * that is legal .dov, so keep -Wunused-function quiet under -Wall -Werror. */
#if defined(__GNUC__) || defined(__clang__)
#define DOV_UNUSED __attribute__((unused))
#else
#define DOV_UNUSED
#endif

/* A string: `len` bytes at `p`, followed by a NUL so C can read it too.
 * The bytes are immutable and never freed (v0.3). */
typedef struct {
    const char *p;
    int64_t len;
} dov_str;
#define DOV_S(lit, n) ((dov_str){(lit), (n)})

/* A slice `[T]`: `len` items starting at item `off` of an immutable heap buffer.
 * `append` may extend the buffer in place, but only past every existing view. */
typedef struct dov_buf {
    int64_t used;       /* items written; views never reach past this */
    int64_t cap;
    max_align_t data[]; /* items follow, suitably aligned */
} dov_buf;
typedef struct {
    dov_buf *buf;
    int64_t off;
    int64_t len;
} dov_slice;
#define dov_slice_empty() ((dov_slice){NULL, 0, 0})
#define DOV_AT(T, s, i, line) (*(const T *)dov_index((s), (i), sizeof(T), (line)))

typedef void (*dov_handler_fn)(void *self, void *args);

void *dov_alloc(size_t size);
void dov_send(void *agent, dov_handler_fn fn, void *args);
void dov_run(void);
_Noreturn void dov_panic(const char *msg, int32_t line);
_Noreturn void dov_panic_str(dov_str msg, int32_t line);


/* Checked arithmetic and bounds checks are inline so the C compiler can optimize them. */
static inline int64_t dov_add(int64_t a, int64_t b, int32_t line) {
    int64_t r;
    if (__builtin_add_overflow(a, b, &r)) dov_panic("integer overflow in `+`", line);
    return r;
}
static inline int64_t dov_sub(int64_t a, int64_t b, int32_t line) {
    int64_t r;
    if (__builtin_sub_overflow(a, b, &r)) dov_panic("integer overflow in `-`", line);
    return r;
}
static inline int64_t dov_mul(int64_t a, int64_t b, int32_t line) {
    int64_t r;
    if (__builtin_mul_overflow(a, b, &r)) dov_panic("integer overflow in `*`", line);
    return r;
}
static inline int64_t dov_div(int64_t a, int64_t b, int32_t line) {
    if (b == 0) dov_panic("division by zero", line);
    if (a == INT64_MIN && b == -1) dov_panic("integer overflow in `/`", line);
    return a / b;
}
static inline int64_t dov_rem(int64_t a, int64_t b, int32_t line) {
    if (b == 0) dov_panic("remainder by zero", line);
    if (b == -1) return 0; /* also avoids INT64_MIN % -1, which traps in C */
    return a % b;
}
static inline int64_t dov_neg(int64_t a, int32_t line) {
    if (a == INT64_MIN) dov_panic("integer overflow in unary `-`", line);
    return -a;
}
static inline int64_t dov_str_at(dov_str s, int64_t i, int32_t line) {
    if (i < 0 || i >= s.len) dov_panic("string index out of bounds", line);
    return (int64_t)(unsigned char)s.p[i];
}
/* Address of item `i` of a slice (bounds-checked). */
static inline const void *dov_index(dov_slice s, int64_t i, size_t size, int32_t line) {
    if (i < 0 || i >= s.len) dov_panic("slice index out of bounds", line);
    return (const char *)s.buf->data + (size_t)(s.off + i) * size;
}

bool dov_str_eq(dov_str a, dov_str b);
dov_str dov_str_range(dov_str s, int64_t a, int64_t b, int32_t line);
dov_str dov_str_concat(dov_str a, dov_str b);
dov_str dov_i64_to_str(int64_t v);
dov_str dov_bool_to_str(bool v);
dov_str dov_char_of(int64_t c, int32_t line);
dov_str dov_str_from_c(const char *p);

dov_slice dov_slice_lit(int64_t n, size_t size, const void *items);
dov_slice dov_range(dov_slice s, int64_t a, int64_t b, int32_t line);
dov_slice dov_append(dov_slice s, size_t size, const void *item);
dov_slice dov_concat(dov_slice a, dov_slice b, size_t size);

void dov_print_i64(int64_t v);
void dov_print_bool(bool v);
void dov_print_str(dov_str v);
void dov_print_nl(void);

#endif
