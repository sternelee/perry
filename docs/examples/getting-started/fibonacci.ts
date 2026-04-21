// demonstrates: recursive fib as a perf-vs-node talking point
// docs: docs/src/getting-started/hello-world.md
// platforms: macos, linux, windows

function fibonacci(n: number): number {
    if (n <= 1) return n
    return fibonacci(n - 1) + fibonacci(n - 2)
}

const start = Date.now()
const result = fibonacci(35)
const elapsed = Date.now() - start

console.log(`fibonacci(35) = ${result}`)
console.log(`Completed in ${elapsed}ms`)
