// JSON parse + stringify polyglot benchmark — Kotlin (kotlinx.serialization).
// 10k records, ~1 MB blob, 50 iterations.
// IDENTICAL workload to bench.ts / bench.go / bench.rs / bench.swift /
// bench.cpp / bench.js.
//
// Library: kotlinx.serialization-json — the de facto standard JSON
// library for Kotlin. Uses the Kotlin compiler plugin to generate
// (de)serializers from @Serializable data classes at compile time.
//
// Build: see run.sh — kotlinc with -Xplugin=...kotlinx-serialization-compiler-plugin.jar
//        and the kotlinx-serialization-{core,json}-jvm JARs on the classpath.

import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json

@Serializable
data class Nested(val x: Int, val y: Int)

@Serializable
data class Item(
    val id: Int,
    val name: String,
    val value: Double,
    val tags: List<String>,
    val nested: Nested,
)

fun main() {
    val items = (0 until 10_000).map { i ->
        Item(
            id = i,
            name = "item_$i",
            value = i * 3.14159,
            tags = listOf("tag_${i % 10}", "tag_${i % 5}"),
            nested = Nested(x = i, y = i * 2),
        )
    }
    val blob: String = Json.encodeToString(items)

    // Warmup — JIT-friendly, charges JVM startup separately.
    repeat(3) {
        val parsed = Json.decodeFromString<List<Item>>(blob)
        Json.encodeToString(parsed)
    }

    val iterations = 50
    val start = System.currentTimeMillis()

    var checksum = 0L
    repeat(iterations) {
        val parsed = Json.decodeFromString<List<Item>>(blob)
        checksum += parsed.size.toLong()
        val reStringified = Json.encodeToString(parsed)
        checksum += reStringified.length.toLong()
    }

    val elapsed = System.currentTimeMillis() - start
    println("ms:$elapsed")
    println("checksum:$checksum")
}
