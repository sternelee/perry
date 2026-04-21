# Utilities

Perry natively implements common utility packages.

## lodash

```typescript,no-test
import _ from "lodash";

_.chunk([1, 2, 3, 4, 5], 2);     // [[1,2], [3,4], [5]]
_.uniq([1, 2, 2, 3, 3]);          // [1, 2, 3]
_.groupBy(users, "role");
_.sortBy(users, ["name"]);
_.cloneDeep(obj);
_.merge(defaults, overrides);
_.debounce(fn, 300);
_.throttle(fn, 100);
```

## dayjs

```typescript,no-test
import dayjs from "dayjs";

const now = dayjs();
console.log(now.format("YYYY-MM-DD"));
console.log(now.add(7, "day").format("YYYY-MM-DD"));
console.log(now.subtract(1, "month").toISOString());

const diff = dayjs("2025-12-31").diff(now, "day");
console.log(`${diff} days until end of year`);
```

## moment

```typescript,no-test
import moment from "moment";

const now = moment();
console.log(now.format("MMMM Do YYYY"));
console.log(now.fromNow());
console.log(moment("2025-01-01").isBefore(now));
```

## uuid

```typescript,no-test
import { v4 as uuidv4 } from "uuid";

const id = uuidv4();
console.log(id); // e.g., "550e8400-e29b-41d4-a716-446655440000"
```

## nanoid

```typescript,no-test
import { nanoid } from "nanoid";

const id = nanoid();       // Default 21 chars
const short = nanoid(10);  // Custom length
console.log(id);
```

## slugify

```typescript,no-test
import slugify from "slugify";

const slug = slugify("Hello World!", { lower: true });
console.log(slug); // "hello-world"
```

## validator

```typescript,no-test
import validator from "validator";

validator.isEmail("test@example.com");  // true
validator.isURL("https://example.com"); // true
validator.isUUID(id);                   // true
validator.isEmpty("");                  // true
```

## Next Steps

- [Other Modules](other.md)
- [Overview](overview.md) — All stdlib modules
