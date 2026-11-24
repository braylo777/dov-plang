#include <stdio.h>
#include <stdint.h>
#include <stdbool.h>
static bool is_prime(int64_t n) { if (n < 2) return false; for (int64_t d = 2; d * d <= n; d++) if (n % d == 0) return false; return true; }
int main(void) { int64_t c = 0; for (int64_t n = 0; n < 300000; n++) if (is_prime(n)) c++; printf("%lld\n", (long long)c); return 0; }
