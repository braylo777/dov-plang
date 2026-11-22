/* .dov pLang runtime v0.2 — agent mailboxes + checked arithmetic.
 * Generated code includes only this header. It declares no libc
 * functions, so `extern "C"` declarations in .dov code cannot clash. */
#ifndef DOV_RT_H
#define DOV_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

_Static_assert(sizeof(long) == 8, "dov v0 targets LP64 (64-bit long)");

/* Generated static functions may be unused (an unsent handler, an uncalled fn);
 * that is legal .dov, so keep -Wunused-function quiet under -Wall -Werror. */
#if defined(__GNUC__) || defined(__clang__)
#define DOV_UNUSED __attribute__((unused))
#else
#define DOV_UNUSED
#endif

typedef void (*dov_handler_fn)(void *self, void *args);

void *dov_alloc(size_t size);
void dov_send(void *agent, dov_handler_fn fn, void *args);
void dov_run(void);
void dov_panic(const char *msg, int32_t line);

int64_t dov_add(int64_t a, int64_t b, int32_t line);
int64_t dov_sub(int64_t a, int64_t b, int32_t line);
int64_t dov_mul(int64_t a, int64_t b, int32_t line);
int64_t dov_div(int64_t a, int64_t b, int32_t line);
int64_t dov_rem(int64_t a, int64_t b, int32_t line);
int64_t dov_neg(int64_t a, int32_t line);
bool dov_str_eq(const char *a, const char *b);

void dov_print_i64(int64_t v);
void dov_print_bool(bool v);
void dov_print_str(const char *v);
void dov_print_nl(void);

#endif
