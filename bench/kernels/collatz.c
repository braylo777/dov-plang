#include <stdio.h>
#include <stdint.h>
static int64_t steps(int64_t n) { int64_t s = 0; while (n != 1) { n = (n % 2 == 0) ? n / 2 : 3 * n + 1; s++; } return s; }
int main(void) { int64_t best = 0, arg = 0; for (int64_t i = 1; i < 500000; i++) { int64_t s = steps(i); if (s > best) { best = s; arg = i; } } printf("%lld %lld\n", (long long)arg, (long long)best); return 0; }
