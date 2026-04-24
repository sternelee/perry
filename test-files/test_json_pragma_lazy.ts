/** @perry-lazy */
// Module-level @perry-lazy JSDoc pragma: every JSON.parse call in
// this file routes through the lazy tape path at codegen time. No
// PERRY_JSON_TAPE=1 env var required. Semantically identical to
// plain JSON.parse — Node erases the JSDoc comment.

interface Record {
  id: number;
  name: string;
  score: number;
}

const blob = '[{"id":1,"name":"alpha","score":9.5},{"id":2,"name":"beta","score":8.25}]';

const parsed = JSON.parse(blob) as Record[];

// Only-length + stringify access: lazy wins massively, stringify
// takes the memcpy fast path since materialized is still null.
console.log("len:" + parsed.length);
console.log("round:" + JSON.stringify(parsed).length);

// Indexed access + field reads: force-materialize kicks in, then
// reads are on the real tree. Must still match Node byte-for-byte.
console.log("0.id:" + parsed[0].id);
console.log("1.name:" + parsed[1].name);
console.log("1.score:" + parsed[1].score);

// .length after materialize — authoritative from tree.
console.log("len-after:" + parsed.length);

// Second stringify — materialized != null now, so walks the tree
// via redirect_lazy_to_materialized. Produces identical bytes.
const r2 = JSON.stringify(parsed);
console.log("r2-len:" + r2.length);
