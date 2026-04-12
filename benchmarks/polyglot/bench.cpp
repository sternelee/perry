#include <chrono>
#include <cstdio>
#include <vector>

using Clock = std::chrono::steady_clock;

inline long long elapsed_ms(Clock::time_point start) {
    return std::chrono::duration_cast<std::chrono::milliseconds>(
        Clock::now() - start).count();
}

int fib(int n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

void bench_fibonacci() {
    auto start = Clock::now();
    int result = fib(40);
    printf("fibonacci:%lld\n", elapsed_ms(start));
    printf("  checksum: %d\n", result);
}

void bench_loop_overhead() {
    auto start = Clock::now();
    double sum = 0.0;
    for (int i = 0; i < 100000000; i++) {
        sum += 1.0;
    }
    printf("loop_overhead:%lld\n", elapsed_ms(start));
    printf("  checksum: %.0f\n", sum);
}

void bench_array_write() {
    std::vector<double> arr(10000000, 0.0);
    auto start = Clock::now();
    for (int i = 0; i < 10000000; i++) {
        arr[i] = static_cast<double>(i);
    }
    printf("array_write:%lld\n", elapsed_ms(start));
    printf("  checksum: %.0f\n", arr[9999999]);
}

void bench_array_read() {
    std::vector<double> arr(10000000);
    for (int i = 0; i < 10000000; i++) {
        arr[i] = static_cast<double>(i);
    }
    auto start = Clock::now();
    double sum = 0.0;
    for (int i = 0; i < 10000000; i++) {
        sum += arr[i];
    }
    printf("array_read:%lld\n", elapsed_ms(start));
    printf("  checksum: %.0f\n", sum);
}

void bench_math_intensive() {
    auto start = Clock::now();
    double result = 0.0;
    for (int i = 1; i <= 50000000; i++) {
        result += 1.0 / static_cast<double>(i);
    }
    printf("math_intensive:%lld\n", elapsed_ms(start));
    printf("  checksum: %.6f\n", result);
}

struct Point {
    double x;
    double y;
};

void bench_object_create() {
    auto start = Clock::now();
    double sum = 0.0;
    for (int i = 0; i < 1000000; i++) {
        Point p{static_cast<double>(i), static_cast<double>(i) * 2.0};
        sum += p.x + p.y;
    }
    printf("object_create:%lld\n", elapsed_ms(start));
    printf("  checksum: %.0f\n", sum);
}

void bench_nested_loops() {
    const int n = 3000;
    std::vector<double> arr(n * n);
    for (int i = 0; i < n * n; i++) {
        arr[i] = static_cast<double>(i);
    }
    auto start = Clock::now();
    double sum = 0.0;
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < n; j++) {
            sum += arr[i * n + j];
        }
    }
    printf("nested_loops:%lld\n", elapsed_ms(start));
    printf("  checksum: %.0f\n", sum);
}

void bench_accumulate() {
    auto start = Clock::now();
    double sum = 0.0;
    for (int i = 0; i < 100000000; i++) {
        sum += static_cast<double>(i % 1000);
    }
    printf("accumulate:%lld\n", elapsed_ms(start));
    printf("  checksum: %.0f\n", sum);
}

int main() {
    bench_fibonacci();
    bench_loop_overhead();
    bench_array_write();
    bench_array_read();
    bench_math_intensive();
    bench_object_create();
    bench_nested_loops();
    bench_accumulate();
    return 0;
}
