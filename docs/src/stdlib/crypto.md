# Cryptography

Perry natively implements password hashing, JWT tokens, and Ethereum cryptography.

## bcrypt

```typescript,no-test
import bcrypt from "bcrypt";

const hash = await bcrypt.hash("mypassword", 10);
const match = await bcrypt.compare("mypassword", hash);
console.log(match); // true
```

## Argon2

```typescript,no-test
import argon2 from "argon2";

const hash = await argon2.hash("mypassword");
const valid = await argon2.verify(hash, "mypassword");
console.log(valid); // true
```

## JSON Web Tokens

```typescript,no-test
import jwt from "jsonwebtoken";

const secret = "my-secret-key";

// Sign a token
const token = jwt.sign({ userId: 123, role: "admin" }, secret, {
  expiresIn: "1h",
});

// Verify a token
const decoded = jwt.verify(token, secret);
console.log(decoded.userId); // 123
```

## Node.js Crypto

```typescript,no-test
import crypto from "crypto";

// Hash
const hash = crypto.createHash("sha256").update("data").digest("hex");

// HMAC
const hmac = crypto.createHmac("sha256", "secret").update("data").digest("hex");

// Random bytes
const bytes = crypto.randomBytes(32);
```

## Ethers

```typescript,no-test
import { ethers } from "ethers";

// Create a wallet
const wallet = ethers.Wallet.createRandom();
console.log(wallet.address);

// Sign a message
const signature = await wallet.signMessage("Hello, Ethereum!");
```

## Next Steps

- [Utilities](utilities.md)
- [Overview](overview.md) — All stdlib modules
