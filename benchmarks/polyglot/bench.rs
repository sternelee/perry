use std::time::Instant;

fn fib(n: i32) -> i32 {
    if n < 2 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn bench_fibonacci() {
    let start = Instant::now();
    let result = fib(40);
    let elapsed = start.elapsed().as_millis();
    println!("fibonacci:{}", elapsed);
    println!("  checksum: {}", result);
}

fn bench_loop_overhead() {
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for _ in 0..100_000_000 {
        sum += 1.0;
    }
    let elapsed = start.elapsed().as_millis();
    println!("loop_overhead:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_array_write() {
    let mut arr = vec![0.0_f64; 10_000_000];
    let start = Instant::now();
    for i in 0..10_000_000 {
        arr[i] = i as f64;
    }
    let elapsed = start.elapsed().as_millis();
    println!("array_write:{}", elapsed);
    println!("  checksum: {:.0}", arr[9_999_999]);
}

fn bench_array_read() {
    let mut arr = vec![0.0_f64; 10_000_000];
    for i in 0..10_000_000 {
        arr[i] = i as f64;
    }
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..10_000_000 {
        sum += arr[i];
    }
    let elapsed = start.elapsed().as_millis();
    println!("array_read:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_math_intensive() {
    let start = Instant::now();
    let mut result: f64 = 0.0;
    for i in 1..=50_000_000 {
        result += 1.0 / i as f64;
    }
    let elapsed = start.elapsed().as_millis();
    println!("math_intensive:{}", elapsed);
    println!("  checksum: {:.6}", result);
}

struct Point {
    x: f64,
    y: f64,
}

fn bench_object_create() {
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..1_000_000 {
        let p = Point {
            x: i as f64,
            y: i as f64 * 2.0,
        };
        sum += p.x + p.y;
    }
    let elapsed = start.elapsed().as_millis();
    println!("object_create:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_nested_loops() {
    let n = 3000;
    let mut arr = vec![0.0_f64; n * n];
    for i in 0..(n * n) {
        arr[i] = i as f64;
    }
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..n {
        for j in 0..n {
            sum += arr[i * n + j];
        }
    }
    let elapsed = start.elapsed().as_millis();
    println!("nested_loops:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_accumulate() {
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..100_000_000_i64 {
        sum += (i % 1000) as f64;
    }
    let elapsed = start.elapsed().as_millis();
    println!("accumulate:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn main() {
    bench_fibonacci();
    bench_loop_overhead();
    bench_array_write();
    bench_array_read();
    bench_math_intensive();
    bench_object_create();
    bench_nested_loops();
    bench_accumulate();
}
