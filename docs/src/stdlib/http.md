# HTTP & Networking

Perry natively implements HTTP servers, clients, and WebSocket support.

## Fastify Server

```typescript,no-test
import fastify from "fastify";

const app = fastify();

app.get("/", async (request, reply) => {
  return { hello: "world" };
});

app.get("/users/:id", async (request, reply) => {
  const { id } = request.params;
  return { id, name: "User " + id };
});

app.post("/data", async (request, reply) => {
  const body = request.body;
  reply.code(201);
  return { received: body };
});

app.listen({ port: 3000 }, () => {
  console.log("Server running on port 3000");
});
```

Perry's Fastify implementation is API-compatible with the npm package. Routes, request/reply objects, params, query strings, and JSON body parsing all work.

## Fetch API

```typescript,no-test
// GET request
const response = await fetch("https://jsonplaceholder.typicode.com/posts/1");
const data = await response.json();

// POST request
const result = await fetch("https://jsonplaceholder.typicode.com/posts", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ title: "hello", body: "world", userId: 1 }),
});
```

## Axios

```typescript,no-test
import axios from "axios";

const { data } = await axios.get("https://jsonplaceholder.typicode.com/users/1");

const response = await axios.post("https://jsonplaceholder.typicode.com/users", {
  name: "Perry",
  email: "perry@example.com",
});
```

## WebSocket

```typescript,no-test
import { WebSocket } from "ws";

const ws = new WebSocket("ws://localhost:8080");

ws.on("open", () => {
  ws.send("Hello, server!");
});

ws.on("message", (data) => {
  console.log(`Received: ${data}`);
});

ws.on("close", () => {
  console.log("Connection closed");
});
```

## Next Steps

- [Databases](database.md)
- [Overview](overview.md) — All stdlib modules
