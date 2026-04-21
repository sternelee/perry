// demonstrates: map / filter / reduce on arrays
// docs: docs/src/stdlib/arrays.md
// platforms: macos, linux, windows

const nums = [1, 2, 3, 4, 5]
const doubled = nums.map((n) => n * 2)
const evens = nums.filter((n) => n % 2 === 0)
const sum = nums.reduce((acc, n) => acc + n, 0)

console.log(doubled.join(","))
console.log(evens.join(","))
console.log(sum)
