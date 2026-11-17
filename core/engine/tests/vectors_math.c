// SPDX-License-Identifier: AGPL-3.0-or-later
// exp, log1p and pow for the engine core's vector tests (vectors.h), in IEEE arithmetic alone, so the cores' results
// do not depend on the platform's libm. These definitions take the place of the C library's in the test executables:
// the Rust core's calls (a curve's shape, a zone's velocity curve) and the C++'s land here. Libms differ in the last bit
// (macOS's pow(x, 2) is not the correctly rounded x * x for 0.13 % of x in 0..1; glibc's is another algorithm), which recorded
// vectors cannot hold. Built -ffp-contract=off. Accuracy is a few ulps: the tests need the same answer everywhere,
// not the best one. oscairn_vectors_math_calls counts them, so a test can tell the definitions took.
#include <math.h>
#include <stdint.h>

uint64_t oscairn_vectors_math_calls[3]; // exp, log1p, pow

static const double ln2Hi = 6.93147180369123816490e-01, ln2Lo = 1.90821492927058770002e-10; // ln2Hi * k is exact for |k| < 2^20

static double twoTo(int k) // 2^k for -1022 <= k <= 1023
{
    union { uint64_t u; double d; } x = { (uint64_t)(k + 1023) << 52 };
    return x.d;
}

static double expOf(double x)
{
    if (x != x) return x;
    if (x > 709.78) return INFINITY;
    if (x < -745.2) return 0;
    const double t = x * 1.44269504088896338700 + (x < 0 ? -.5 : .5);
    const int k = (int)t; // to nearest
    const double r = (x - k * ln2Hi) - k * ln2Lo; // |r| <= 0.35
    double p = 1.0 / 6227020800.0; // 1/13!
    static const double inverse[] = { 1.0 / 479001600.0, 1.0 / 39916800.0, 1.0 / 3628800.0, 1.0 / 362880.0, 1.0 / 40320.0, 1.0 / 5040.0,
                                      1.0 / 720.0, 1.0 / 120.0, 1.0 / 24.0, 1.0 / 6.0, .5, 1.0, 1.0 };
    for (int i = 0; i < 13; ++i) p = p * r + inverse[i];
    if (k < -1020) return p * twoTo(k + 1000) * twoTo(-1000); // the one rounding into the subnormals
    if (k > 1023) return p * twoTo(k - 1) * 2;
    return p * twoTo(k);
}

static double logOf(double x) // x > 0, finite
{
    union { double d; uint64_t u; } v = { x };
    int e = 0;
    if (!(v.u >> 52)) { v.d *= 18014398509481984.0; e = -54; } // a subnormal: scaled by 2^54
    e += (int)(v.u >> 52) - 1022;
    v.u = (v.u & 0x000fffffffffffffull) | 0x3fe0000000000000ull; // m in [0.5, 1)
    if (v.d < 0.70710678118654752440) { v.d *= 2; --e; }
    const double f = v.d - 1, s = f / (2 + f), s2 = s * s; // log(m) = 2 atanh(s), |s| <= 0.1716
    double p = 1.0 / 25;
    for (int n = 23; n >= 1; n -= 2) p = p * s2 + 1.0 / n;
    return e * ln2Hi + (2 * s * p + e * ln2Lo);
}

double exp(double x)
{
    ++oscairn_vectors_math_calls[0];
    return expOf(x);
}

double log1p(double x)
{
    ++oscairn_vectors_math_calls[1];
    if (x != x || x == INFINITY) return x;
    if (x < -1) return NAN;
    if (x == -1) return -INFINITY;
    const double u = 1 + x;
    if (u == 1) return x;
    return logOf(u) + (x - (u - 1)) / u; // what 1 + x lost in rounding, to first order
}

double pow(double x, double y)
{
    ++oscairn_vectors_math_calls[2];
    if (y == 0 || x == 1) return 1;
    if (x != x || y != y) return x + y;
    if (y == 1) return x;
    if (y == 2) return x * x;
    if (x == 0) return y > 0 ? 0 : INFINITY;
    if (x < 0)
    {
        if (y > 9e15 || y < -9e15 || y != (double)(int64_t)y) return NAN;
        const double r = pow(-x, y);
        return (int64_t)y & 1 ? -r : r;
    }
    if (x == INFINITY) return y > 0 ? x : 0;
    return expOf(y * logOf(x));
}
