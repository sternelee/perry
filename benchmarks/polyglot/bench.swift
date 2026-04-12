import Foundation

func benchFibonacci() {
    func fib(_ n: Int) -> Int {
        if n < 2 { return n }
        return fib(n - 1) + fib(n - 2)
    }

    let start = CFAbsoluteTimeGetCurrent()
    let result = fib(40)
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("fibonacci:\(elapsed)")
    print("  checksum: \(result)")
}

func benchLoopOverhead() {
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for _ in 0..<100_000_000 {
        sum += 1.0
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("loop_overhead:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchArrayWrite() {
    var arr = [Double](repeating: 0.0, count: 10_000_000)
    let start = CFAbsoluteTimeGetCurrent()
    for i in 0..<10_000_000 {
        arr[i] = Double(i)
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("array_write:\(elapsed)")
    print("  checksum: \(Int(arr[9_999_999]))")
}

func benchArrayRead() {
    var arr = [Double](repeating: 0.0, count: 10_000_000)
    for i in 0..<10_000_000 {
        arr[i] = Double(i)
    }
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<10_000_000 {
        sum += arr[i]
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("array_read:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchMathIntensive() {
    let start = CFAbsoluteTimeGetCurrent()
    var result: Double = 0.0
    for i in 1...50_000_000 {
        result += 1.0 / Double(i)
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("math_intensive:\(elapsed)")
    print("  checksum: \(String(format: "%.6f", result))")
}

struct Point {
    var x: Double
    var y: Double
}

func benchObjectCreate() {
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<1_000_000 {
        let p = Point(x: Double(i), y: Double(i) * 2.0)
        sum += p.x + p.y
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("object_create:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchNestedLoops() {
    let n = 3000
    var arr = [Double](repeating: 0.0, count: n * n)
    for i in 0..<(n * n) {
        arr[i] = Double(i)
    }
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<n {
        for j in 0..<n {
            sum += arr[i * n + j]
        }
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("nested_loops:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchAccumulate() {
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<100_000_000 {
        sum += Double(i % 1000)
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("accumulate:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

benchFibonacci()
benchLoopOverhead()
benchArrayWrite()
benchArrayRead()
benchMathIntensive()
benchObjectCreate()
benchNestedLoops()
benchAccumulate()
