import time


def bench_fibonacci():
    def fib(n):
        if n < 2:
            return n
        return fib(n - 1) + fib(n - 2)

    start = time.monotonic()
    result = fib(40)
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"fibonacci:{elapsed}")
    print(f"  checksum: {result}")


def bench_loop_overhead():
    start = time.monotonic()
    sum_val = 0.0
    for _ in range(100_000_000):
        sum_val += 1.0
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"loop_overhead:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_array_write():
    arr = [0.0] * 10_000_000
    start = time.monotonic()
    for i in range(10_000_000):
        arr[i] = float(i)
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"array_write:{elapsed}")
    print(f"  checksum: {arr[9_999_999]:.0f}")


def bench_array_read():
    arr = [float(i) for i in range(10_000_000)]
    start = time.monotonic()
    sum_val = 0.0
    for i in range(10_000_000):
        sum_val += arr[i]
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"array_read:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_math_intensive():
    start = time.monotonic()
    result = 0.0
    for i in range(1, 50_000_001):
        result += 1.0 / i
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"math_intensive:{elapsed}")
    print(f"  checksum: {result:.6f}")


class Point:
    __slots__ = ('x', 'y')

    def __init__(self, x, y):
        self.x = x
        self.y = y


def bench_object_create():
    start = time.monotonic()
    sum_val = 0.0
    for i in range(1_000_000):
        p = Point(float(i), float(i) * 2.0)
        sum_val += p.x + p.y
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"object_create:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_nested_loops():
    n = 3000
    arr = [float(i) for i in range(n * n)]
    start = time.monotonic()
    sum_val = 0.0
    for i in range(n):
        for j in range(n):
            sum_val += arr[i * n + j]
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"nested_loops:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_accumulate():
    start = time.monotonic()
    sum_val = 0.0
    for i in range(100_000_000):
        sum_val += i % 1000
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"accumulate:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


if __name__ == "__main__":
    bench_fibonacci()
    bench_loop_overhead()
    bench_array_write()
    bench_array_read()
    bench_math_intensive()
    bench_object_create()
    bench_nested_loops()
    bench_accumulate()
