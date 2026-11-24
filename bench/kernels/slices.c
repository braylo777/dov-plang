#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
int main(void) {
    int64_t cap = 4, len = 0; int64_t *xs = malloc(sizeof *xs * cap);
    for (int64_t i = 0; i < 2000000; i++) { if (len == cap) { cap *= 2; xs = realloc(xs, sizeof *xs * cap); } xs[len++] = i % 1000; }
    int64_t sum = 0; for (int64_t i = 0; i < len; i++) sum += xs[i];
    printf("%lld %lld\n", (long long)len, (long long)sum); free(xs); return 0;
}
