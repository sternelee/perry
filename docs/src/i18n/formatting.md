# Locale-Aware Formatting

Perry provides format wrapper functions that automatically format values according to the current locale. Import them from `perry/i18n`:

```typescript,no-test
import { Currency, Percent, ShortDate, LongDate, FormatNumber, FormatTime, Raw } from "perry/i18n";
```

## Format Wrappers

### Currency

Formats a number as currency with the locale's symbol, decimal separator, and symbol placement:

```typescript,no-test
Text("Total: {price}", { price: Currency(23.10) })
// en: "Total: $23.10"
// de: "Total: 23,10 €"
// fr: "Total: 23,10 €"
// ja: "Total: ¥23.10"
```

### Percent

Formats a decimal as a percentage (value is multiplied by 100):

```typescript,no-test
Text("Discount: {rate}", { rate: Percent(0.15) })
// en: "Discount: 15%"
// de: "Discount: 15 %"
// fr: "Discount: 15 %"
```

### FormatNumber

Formats a number with locale-appropriate grouping and decimal separators:

```typescript,no-test
Text("Population: {n}", { n: FormatNumber(1234567.89) })
// en: "Population: 1,234,567.89"
// de: "Population: 1.234.567,89"
// fr: "Population: 1 234 567,89"
```

### ShortDate / LongDate / FormatDate

Formats a timestamp (milliseconds since epoch) as a date:

```typescript,no-test
const now = Date.now();

Text("Due: {d}", { d: ShortDate(now) })
// en: "Due: 3/22/2026"
// de: "Due: 22.03.2026"
// ja: "Due: 2026/03/22"

Text("Event: {d}", { d: LongDate(now) })
// en: "Event: March 22, 2026"
// de: "Event: 22. März 2026"
// fr: "Event: 22 mars 2026"
```

### FormatTime

Formats a timestamp as time (12h vs 24h based on locale):

```typescript,no-test
Text("At: {t}", { t: FormatTime(timestamp) })
// en: "At: 3:45 PM"
// de: "At: 15:45"
// fr: "At: 15:45"
```

### Raw

Pass-through — prevents any automatic formatting. Use when a parameter name might trigger auto-formatting but you want the raw value:

```typescript,no-test
Text("Code: {amount}", { amount: Raw(12345) })
// All locales: "Code: 12345" (no currency formatting despite the name)
```

## Locale-Specific Formatting Rules

Perry includes hand-rolled formatting rules for 25+ locales:

| Feature | Example Locales |
|---------|----------------|
| Decimal: `.` / Thousands: `,` | en, ja, zh, ko |
| Decimal: `,` / Thousands: `.` | de, nl, tr, es, it, pt |
| Decimal: `,` / Thousands: ` ` (narrow space) | fr |
| Decimal: `,` / Thousands: ` ` (non-breaking space) | ru, uk, pl, sv, da, no, fi |
| Currency before number: `$23.10` | en, ja, zh, ko |
| Currency after number: `23,10 €` | de, fr, es, it, ru |
| Percent with space: `42 %` | de, fr, es, ru |
| Percent without space: `42%` | en, ja, zh |
| Date order: M/D/Y | en |
| Date order: D.M.Y | de, fr, es, ru |
| Date order: Y/M/D | ja, zh, ko, sv |
| 24-hour time | de, fr, es, ja, zh, ru (most) |
| 12-hour time (AM/PM) | en |

## Currency Configuration

Configure default currency codes per locale in `perry.toml`:

```toml
[i18n]
locales = ["en", "de", "fr"]
default_locale = "en"

[i18n.currencies]
en = "USD"
de = "EUR"
fr = "EUR"
```

When `Currency(value)` is called, the locale's configured currency code determines the symbol and formatting rules.
