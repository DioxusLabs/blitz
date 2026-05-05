# Syntax Highlighting Demo

This document is used to verify code-block syntax highlighting in the
`readme` app. Each section below contains a fenced code block tagged with
a language identifier so the highlighter has something to dispatch on.

A plain (untagged) block — should fall back to no highlighting:

```
fn this_block_has_no_language_tag() {
    let x = 42;
}
```

An indented (non-fenced) block — also no language info:

    SELECT * FROM users WHERE id = 1;

---

## Rust

```rust
use std::collections::HashMap;

/// Greet a list of names, returning the count.
pub fn greet(names: &[&str]) -> usize {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for name in names {
        *seen.entry(name).or_insert(0) += 1;
        println!("Hello, {name}!");
    }
    seen.len()
}

#[derive(Debug, Clone)]
struct Point<T: Copy> {
    x: T,
    y: T,
}

impl<T: Copy + std::ops::Add<Output = T>> Point<T> {
    fn translate(self, dx: T, dy: T) -> Self {
        Self { x: self.x + dx, y: self.y + dy }
    }
}
```

## Python

```python
from dataclasses import dataclass
from typing import Iterable

@dataclass(frozen=True)
class User:
    id: int
    name: str
    tags: tuple[str, ...] = ()

def filter_admins(users: Iterable[User]) -> list[User]:
    """Return users tagged 'admin'."""
    return [u for u in users if "admin" in u.tags]

if __name__ == "__main__":
    sample = [User(1, "ada", ("admin",)), User(2, "bob")]
    print(filter_admins(sample))
```

## JavaScript

```javascript
// Debounce: returns a wrapped fn that fires after `ms` of quiet.
function debounce(fn, ms = 250) {
  let t;
  return (...args) => {
    clearTimeout(t);
    t = setTimeout(() => fn.apply(this, args), ms);
  };
}

const onResize = debounce(() => {
  console.log(`viewport: ${window.innerWidth}x${window.innerHeight}`);
}, 100);

window.addEventListener("resize", onResize);
```

## TypeScript

```typescript
type Result<T, E = Error> =
  | { ok: true; value: T }
  | { ok: false; error: E };

async function fetchJson<T>(url: string): Promise<Result<T>> {
  try {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return { ok: true, value: (await res.json()) as T };
  } catch (error) {
    return { ok: false, error: error as Error };
  }
}
```

## Go

```go
package main

import (
    "context"
    "fmt"
    "time"
)

func slowEcho(ctx context.Context, msg string) (string, error) {
    select {
    case <-time.After(500 * time.Millisecond):
        return msg, nil
    case <-ctx.Done():
        return "", ctx.Err()
    }
}

func main() {
    ctx, cancel := context.WithTimeout(context.Background(), time.Second)
    defer cancel()
    out, err := slowEcho(ctx, "hello")
    fmt.Println(out, err)
}
```

## C

```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} Buf;

int buf_push(Buf *b, char c) {
    if (b->len == b->cap) {
        size_t cap = b->cap ? b->cap * 2 : 16;
        char *p = realloc(b->data, cap);
        if (!p) return -1;
        b->data = p;
        b->cap = cap;
    }
    b->data[b->len++] = c;
    return 0;
}
```

## C++

```cpp
#include <algorithm>
#include <iostream>
#include <ranges>
#include <vector>

template <std::ranges::range R>
auto sum_squares(const R& xs) {
    return std::ranges::fold_left(
        xs | std::views::transform([](auto x) { return x * x; }),
        0, std::plus{});
}

int main() {
    std::vector v{1, 2, 3, 4};
    std::cout << sum_squares(v) << '\n';  // 30
}
```

## Java

```java
import java.util.List;
import java.util.stream.Collectors;

public sealed interface Shape permits Circle, Square {}
record Circle(double r) implements Shape {}
record Square(double s) implements Shape {}

public class Areas {
    public static double total(List<Shape> shapes) {
        return shapes.stream().mapToDouble(s -> switch (s) {
            case Circle c -> Math.PI * c.r() * c.r();
            case Square sq -> sq.s() * sq.s();
        }).sum();
    }
}
```

## Bash

```bash
#!/usr/bin/env bash
set -euo pipefail

# Roll a simple log archiver.
log_dir="${1:-/var/log}"
out="${HOME}/logs-$(date +%Y%m%d).tar.gz"

if [[ ! -d "$log_dir" ]]; then
  echo "no such dir: $log_dir" >&2
  exit 1
fi

tar -czf "$out" -C "$log_dir" .
echo "wrote ${out} ($(du -h "$out" | cut -f1))"
```

## JSON

```json
{
  "name": "blitz-readme",
  "version": "0.1.0",
  "private": true,
  "features": ["gpu", "comrak", "floats"],
  "deps": {
    "comrak": "0.52",
    "syntect": { "version": "5", "default-features": false }
  },
  "enabled": true,
  "ratio": 1.5,
  "notes": null
}
```

## YAML

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        rust: [stable, beta]
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --all-features
        env:
          RUST_BACKTRACE: 1
```

## TOML

```toml
[package]
name = "readme"
version = "0.1.0"
edition = "2024"

[features]
default = ["gpu", "comrak"]
syntax-highlight = ["comrak/syntect"]

[dependencies.syntect]
version = "5"
default-features = false
features = ["default-fancy"]
```

## HTML

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Demo</title>
    <style>
      body { font-family: system-ui; }
    </style>
  </head>
  <body>
    <main id="root" data-loaded="false">
      <h1>Hello &amp; welcome</h1>
      <!-- mounted by JS -->
    </main>
  </body>
</html>
```

## CSS

```css
:root {
  --accent: #6366f1;
}

.markdown-body pre {
  padding: 1rem;
  border-radius: 6px;
  background: #0d1117;
  color: #c9d1d9;
}

@media (prefers-color-scheme: light) {
  .markdown-body pre {
    background: #f6f8fa;
    color: #24292f;
  }
}
```

## SQL

```sql
WITH recent_orders AS (
  SELECT user_id, COUNT(*) AS n, MAX(created_at) AS last_at
  FROM orders
  WHERE created_at > NOW() - INTERVAL '30 days'
  GROUP BY user_id
)
SELECT u.id, u.email, r.n, r.last_at
FROM users u
LEFT JOIN recent_orders r ON r.user_id = u.id
WHERE u.active IS TRUE
ORDER BY r.n DESC NULLS LAST
LIMIT 50;
```

## Ruby

```ruby
class Greeter
  attr_reader :name

  def initialize(name)
    @name = name
  end

  def greet(times: 1)
    times.times.map { |i| "(#{i + 1}) hello, #{@name}!" }
  end
end

puts Greeter.new("world").greet(times: 3)
```

## Diff

```diff
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,8 @@
-fn main() {
-    println!("hello");
+use std::env;
+
+fn main() {
+    let who = env::args().nth(1).unwrap_or_else(|| "world".into());
+    println!("hello, {who}");
 }
```

## Inline code

This paragraph has `inline code` and a longer span like
`let mut counter: u32 = 0;` that should not be highlighted as a block.

---

## Edge cases

A block tagged with an unknown language — highlighter should leave the
text untouched (or fall back to plain):

```nonsense-lang-xyz
this is not a real language;
nothing should crash.
```

A block with extra info-string metadata (some markdown processors pass
this through):

```rust,ignore
fn ignored_in_doctests() { /* ... */ }
```

A very short block:

```sh
ls -la
```
