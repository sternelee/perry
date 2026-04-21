# Interpolation & Plurals

## Parameterized Strings

Use `{param}` placeholders in your strings and pass values as a second argument:

```typescript,no-test
import { Text } from "perry/ui";

Text("Hello, {name}!", { name: user.name })
Text("Total: {price}", { price: order.total })
```

Translation files use the same `{param}` syntax:

```json
// locales/en.json
{
  "Hello, {name}!": "Hello, {name}!",
  "Total: {price}": "Total: {price}"
}

// locales/de.json
{
  "Hello, {name}!": "Hallo, {name}!",
  "Total: {price}": "Gesamt: {price}"
}
```

Parameters are substituted at runtime after the locale-appropriate template is selected. The substitution handles any value type (numbers, strings, dates) by converting to string.

### Compile-Time Validation

Perry validates parameters across all locales during compilation:

| Condition | Severity |
|-----------|----------|
| `{param}` in translation but not provided in code | Error |
| Param in code but `{param}` not in translation | Error |
| Parameter set differs between locales for same key | Error |

## Plural Rules

Plural forms use dot-suffix keys based on CLDR plural categories: `.zero`, `.one`, `.two`, `.few`, `.many`, `.other`.

### Locale Files

```json
// locales/en.json
{
  "You have {count} items.one": "You have {count} item.",
  "You have {count} items.other": "You have {count} items."
}

// locales/de.json
{
  "You have {count} items.one": "Du hast {count} Artikel.",
  "You have {count} items.other": "Du hast {count} Artikel."
}

// locales/pl.json (Polish: one, few, many)
{
  "You have {count} items.one": "Masz {count} element.",
  "You have {count} items.few": "Masz {count} elementy.",
  "You have {count} items.many": "Masz {count} elementow.",
  "You have {count} items.other": "Masz {count} elementu."
}
```

### Usage in Code

Reference the base key without any suffix. Perry detects the plural variants automatically:

```typescript,no-test
Text("You have {count} items", { count: cart.items.length })
```

Perry determines which plural form to use based on the `count` parameter value and the current locale's CLDR rules.

### Supported Locales

Perry includes hand-rolled CLDR plural rules for 30+ locales:

| Pattern | Locales |
|---------|---------|
| one/other | English, German, Dutch, Swedish, Danish, Norwegian, Finnish, Estonian, Hungarian, Turkish, Greek, Hebrew, Italian, Spanish, Portuguese, Catalan, Bulgarian, Hindi, Bengali, Swahili, ... |
| one (0-1) / other | French |
| no distinction | Japanese, Chinese, Korean, Vietnamese, Thai, Indonesian, Malay |
| one/few/many | Russian, Ukrainian, Serbian, Croatian, Bosnian, Polish |
| one/few/other | Czech, Slovak |
| zero/one/two/few/many/other | Arabic |
| one/few/other | Romanian, Lithuanian |
| zero/one/other | Latvian |

### Compile-Time Validation

| Condition | Severity |
|-----------|----------|
| `.other` form missing for any locale | Error |
| Required CLDR category missing (e.g., `.few` for Polish) | Error |
| Extra category locale doesn't use (e.g., `.few` for English) | Warning |

## Explicit API for Non-UI Strings

For strings outside UI components (API responses, notifications, etc.), use `t()`:

```typescript,no-test
import { t } from "perry/i18n";

const message = t("Your order has been shipped.");
const welcome = t("Welcome back, {name}!", { name: user.name });
```

This uses the same key lookup, validation, and interpolation as UI strings.
