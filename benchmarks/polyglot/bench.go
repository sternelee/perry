package main

import (
	"fmt"
	"time"
)

func benchFibonacci() {
	var fib func(n int) int
	fib = func(n int) int {
		if n < 2 {
			return n
		}
		return fib(n-1) + fib(n-2)
	}

	start := time.Now()
	result := fib(40)
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("fibonacci:%d\n", elapsed)
	fmt.Printf("  checksum: %d\n", result)
}

func benchLoopOverhead() {
	start := time.Now()
	sum := 0.0
	for i := 0; i < 100_000_000; i++ {
		sum += 1.0
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("loop_overhead:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchArrayWrite() {
	arr := make([]float64, 10_000_000)
	start := time.Now()
	for i := 0; i < 10_000_000; i++ {
		arr[i] = float64(i)
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("array_write:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", arr[9_999_999])
}

func benchArrayRead() {
	arr := make([]float64, 10_000_000)
	for i := 0; i < 10_000_000; i++ {
		arr[i] = float64(i)
	}
	start := time.Now()
	sum := 0.0
	for i := 0; i < 10_000_000; i++ {
		sum += arr[i]
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("array_read:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchMathIntensive() {
	start := time.Now()
	result := 0.0
	for i := 1; i <= 50_000_000; i++ {
		result += 1.0 / float64(i)
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("math_intensive:%d\n", elapsed)
	fmt.Printf("  checksum: %.6f\n", result)
}

type Point struct {
	x float64
	y float64
}

func benchObjectCreate() {
	start := time.Now()
	sum := 0.0
	for i := 0; i < 1_000_000; i++ {
		p := Point{x: float64(i), y: float64(i) * 2.0}
		sum += p.x + p.y
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("object_create:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchNestedLoops() {
	n := 3000
	arr := make([]float64, n*n)
	for i := 0; i < n*n; i++ {
		arr[i] = float64(i)
	}
	start := time.Now()
	sum := 0.0
	for i := 0; i < n; i++ {
		for j := 0; j < n; j++ {
			sum += arr[i*n+j]
		}
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("nested_loops:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchAccumulate() {
	start := time.Now()
	sum := 0.0
	for i := 0; i < 100_000_000; i++ {
		sum += float64(i % 1000)
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("accumulate:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func main() {
	benchFibonacci()
	benchLoopOverhead()
	benchArrayWrite()
	benchArrayRead()
	benchMathIntensive()
	benchObjectCreate()
	benchNestedLoops()
	benchAccumulate()
}
