# Databases

Perry natively implements clients for MySQL, PostgreSQL, SQLite, MongoDB, and Redis.

## MySQL

```typescript,no-test
import mysql from "mysql2/promise";

const connection = await mysql.createConnection({
  host: "localhost",
  user: "root",
  password: "password",
  database: "mydb",
});

const [rows] = await connection.execute("SELECT * FROM users WHERE id = ?", [1]);
console.log(rows);

await connection.end();
```

## PostgreSQL

```typescript,no-test
import { Client } from "pg";

const client = new Client({
  host: "localhost",
  port: 5432,
  user: "postgres",
  password: "password",
  database: "mydb",
});

await client.connect();
const result = await client.query("SELECT * FROM users WHERE id = $1", [1]);
console.log(result.rows);
await client.end();
```

## SQLite

```typescript,no-test
import Database from "better-sqlite3";

const db = new Database("mydb.sqlite");

db.exec(`
  CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    name TEXT,
    email TEXT
  )
`);

const insert = db.prepare("INSERT INTO users (name, email) VALUES (?, ?)");
insert.run("Perry", "perry@example.com");

const users = db.prepare("SELECT * FROM users").all();
console.log(users);
```

## MongoDB

```typescript,no-test
import { MongoClient } from "mongodb";

const client = new MongoClient("mongodb://localhost:27017");
await client.connect();

const db = client.db("mydb");
const users = db.collection("users");

await users.insertOne({ name: "Perry", email: "perry@example.com" });
const user = await users.findOne({ name: "Perry" });
console.log(user);

await client.close();
```

## Redis

```typescript,no-test
import Redis from "ioredis";

const redis = new Redis();

await redis.set("key", "value");
const value = await redis.get("key");
console.log(value); // "value"

await redis.del("key");
await redis.quit();
```

## Next Steps

- [Cryptography](crypto.md)
- [Overview](overview.md) — All stdlib modules
