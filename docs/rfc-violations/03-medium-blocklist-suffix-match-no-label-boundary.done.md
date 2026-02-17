# Blocklist Suffix Match Without Label Boundary Check

## Severity: Medium-High

## Location

`src/dns.rs:107-109`

```rust
for it in &self.blacklist {
    if domain.ends_with(it) {
        checked.block.insert(domain.to_string());
```

`src/blocklist.rs:20`

```rust
.map(|it| format!("{}.", it))
```

## Violated Standards

- DNS name-space correctness (not a specific RFC violation, but a domain matching correctness issue)

## Description

The blocklist matching uses `String::ends_with` for suffix comparison, which operates at the byte/character level rather than the DNS label level. For example, if the blocklist contains `ad.com.`, a query for `bad.com.` would match because `"bad.com."` ends with `"ad.com."`. This results in false positives that block legitimate domains.

Correct DNS suffix matching must respect label boundaries: a match should only succeed if the blocklist entry matches the full domain or is preceded by a `.` character.

## Impact

Legitimate domains may be incorrectly blocked depending on the blocklist content. For example, `ad.com.` in the blocklist would also block `bad.com.`, `mad.com.`, `chad.com.`, etc.

## Suggested Fix

Change the matching logic to verify label boundaries. For example:

```rust
if domain == it || domain.ends_with(&format!(".{}", it)) {
    // matched on label boundary
}
```
