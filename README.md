# opaque-enum

[![crates.io](https://img.shields.io/crates/v/opaque-enum.svg)](https://crates.io/crates/opaque-enum)
[![docs.rs](https://docs.rs/opaque-enum/badge.svg)](https://docs.rs/opaque-enum)
[![CI](https://github.com/veecore/opaque-enum/actions/workflows/ci.yml/badge.svg)](https://github.com/veecore/opaque-enum/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

You know the drill. You write a nice `pub enum MyError { ... }` in your library, ship it, and two weeks later you want to add a new variant. Suddenly every downstream crate that pattern-matched your error is broken. Semver bump time — just for adding a variant.

The usual fix is wrapping the enum in a struct and making the inner type private. That solves the API problem, but now you're writing pages of boilerplate just to forward `Display`, `Error::source`, and a handful of methods through the wrapper.

`opaque-enum` is that boilerplate, automated.

```toml
[dependencies]
opaque-enum = "0.1"
```

---

## The Basic Idea

Stick `#[opaque_enum]` on your enum and it becomes a struct. Same name, same public interface, but the variants are hidden. You can add, remove, or restructure them without touching your semver.

Then put the same attribute on any `impl` block and write your code exactly as if the variants were still public — `match self`, `Self::Variant`, all of it. The macro rewrites the internals.

```rust
use opaque_enum::opaque_enum;
use std::fmt::{self, Display, Formatter};
use std::error::Error;

#[opaque_enum(wrapper = Box)]
#[derive(Debug)]
pub enum DatabaseError {
    ConnectionFailed(std::io::Error),
    QueryFailed {
        query: String,
        reason: String,
    },
    PermissionDenied,
}

#[opaque_enum]
impl DatabaseError {
    pub fn is_connection_error(&self) -> bool {
        matches!(self, Self::ConnectionFailed(_))
    }
}

#[opaque_enum]
impl Display for DatabaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed(err) => write!(f, "connection failed: {err}"),
            Self::QueryFailed { query, reason } => write!(f, "query `{query}` failed: {reason}"),
            Self::PermissionDenied => write!(f, "permission denied"),
        }
    }
}

#[opaque_enum]
impl Error for DatabaseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ConnectionFailed(err) => Some(err),
            Self::QueryFailed { .. } | Self::PermissionDenied => None,
        }
    }
}
```

That's the whole pattern. The `DatabaseError` your users see is an opaque struct. The variants are yours to change freely.

---

## Things Worth Knowing

**`wrapper = Box`** — When your enum has large variants, boxing the inner representation keeps `sizeof(DatabaseError) == sizeof(*mut ())`. Worth it for errors that rarely get created but pass through a lot of code.

**Constructor visibility** — If your enum is `pub`, the generated constructors (like `DatabaseError::ConnectionFailed(...)`) are silently demoted to `pub(crate)`. External code can't construct your variants directly, which is the whole point.

**`Pin` receivers** — `self: Pin<&Self>` and `self: Pin<&mut Self>` are supported via the `OpaqueProject` trait.

---

## How It Works Under the Hood

Given `pub enum Foo { ... }`, the macro:

1. Renames the enum to `enum FooInner` and strips it of any visibility.
2. Emits a `pub struct Foo { inner: FooInner }` (or `Box<FooInner>` with `wrapper = Box`).
3. Generates a constructor function for each variant — same name, same fields, `pub(crate)` visibility.
4. Implements `From<FooInner>` and the `OpaqueProject` projection trait.
5. For each `#[opaque_enum] impl` block, duplicates the impl targeting `FooInner` (the real implementation) and emits a forwarding impl on `Foo` that projects `self` before each call.

---

## Known Rough Edges

### Composite return types containing `Self`

The macro can rewrite `-> Self` and wrap the result with `Into::into`. It cannot yet handle `-> Option<Self>`, `-> Result<Self, E>`, or any other composite. Those will produce a type mismatch at compile time.

```rust
// ❌ won't compile
#[opaque_enum]
impl MyEnum {
    fn maybe(self) -> Option<Self> { Some(self) }
}
```

The workaround is to build the value using the generated constructors instead of relying on the implicit conversion:

```rust
// ✅ construct explicitly
#[opaque_enum]
impl MyEnum {
    fn maybe(self) -> Option<Self> {
        match self {
            Self::Variant(x) => Some(Self::Variant(x)),
            _ => None,
        }
    }
}
```

An `InverseProject` trait to automate this is planned.

### `-> &Self` return types

Returning a reference to self doesn't work yet. The inner call returns `&FooInner` but the wrapper signature expects `&Foo`. This will need a transmute-based projection and is left for a future release.

### Calling wrapper-only methods from inside decorated blocks

Each `#[opaque_enum] impl` block is internally rewritten to target `FooInner`, not `Foo`. That means `self.some_method()` inside a decorated block resolves against `FooInner`. If `some_method` is only defined on the outer `Foo` wrapper (e.g. in an un-decorated `impl Foo`), it won't be found.

The fix is straightforward: put any method you intend to call from inside a decorated block into its own decorated block.

---

## License

MIT OR Apache-2.0, at your option.
