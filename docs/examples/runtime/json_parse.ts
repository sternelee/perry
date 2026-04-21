// demonstrates: JSON.parse + JSON.stringify roundtrip
// docs: docs/src/stdlib/json.md
// platforms: macos, linux, windows

const input = '{"name":"perry","version":3}'
const parsed = JSON.parse(input) as { name: string; version: number }
console.log(parsed.name)
console.log(parsed.version)
console.log(JSON.stringify(parsed))
