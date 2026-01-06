# objc2 0.5 → 0.6 Migration Guide

A comprehensive guide based on practical experience porting Swift/Objective-C code to Rust using objc2 0.6+.

## Table of Contents
- [Major Changes Overview](#major-changes-overview)
- [Memory Management](#memory-management)
- [Class Declaration](#class-declaration)
- [Method Calls](#method-calls)
- [Collections](#collections)
- [Foundation Types](#foundation-types)
- [Common Patterns](#common-patterns)
- [Troubleshooting](#troubleshooting)

---

## Major Changes Overview

### What Changed in objc2 0.6

1. **`Retained<T>` replaces `Id<T>`**
   - All owned Objective-C objects now use `Retained<T>`
   - Better semantics around object ownership

2. **Simplified `unsafe` usage**
   - Many APIs that were safe are now exposed without `unsafe`
   - Framework-specific safe wrappers (e.g., `NSApplication::sharedApplication()`)

3. **`ClassType` trait method changes**
   - `::alloc()` now requires the trait to be in scope
   - Use `MainThreadMarker::alloc::<T>()` for main-thread classes

4. **`NSArray` API changes**
   - `from_vec()` → `from_slice()`
   - Requires references now, not owned values

5. **Auto-generated bindings**
   - `objc2-virtualization`, `objc2-app-kit`, etc. are now auto-generated
   - Consistent API across all frameworks

---

## Memory Management

### Retained vs Id

**objc2 0.5:**
```rust
use objc2::rc::Id;

let obj: Id<NSObject> = unsafe { NSObject::new() };
```

**objc2 0.6:**
```rust
use objc2::rc::Retained;

let obj: Retained<NSObject> = unsafe { NSObject::new() };
```

### Object Allocation

**objc2 0.5:**
```rust
use objc2::ClassType;

let obj = unsafe { MyClass::alloc() };
```

**objc2 0.6:**
```rust
use objc2::ClassType;  // Must import trait for ::alloc()

let obj = unsafe { MyClass::alloc() };
```

**For Main-Thread Classes:**
```rust
use objc2_foundation::MainThreadMarker;

let mtm = MainThreadMarker::new().unwrap();
let obj = mtm.alloc::<NSWindow>();  // Preferred for AppKit classes
```

### Casting Between Types

**Downcasting (child → parent):**
```rust
// objc2 0.5
let parent: Retained<Parent> = unsafe { Retained::cast(child) };

// objc2 0.6
let parent: Retained<Parent> = child.downcast().unwrap();
```

**Upcasting (to super type):**
```rust
let parent_ref: &Parent = child.as_super();
```

---

## Class Declaration

### Basic Class Declaration

**objc2 0.5:**
```rust
objc2::declare_class!(
    struct MyClass;

    unsafe impl ClassType for MyClass {
        type Super = NSObject;
        const NAME: &'static str = "MyClass";
    }
);
```

**objc2 0.6:**
```rust
use objc2::declare_class;
use objc2::mutability::MainThreadOnly;  // If needed

declare_class!(
    struct MyClass;

    unsafe impl ClassType for MyClass {
        type Super = NSObject;
        type Mutability = MainThreadOnly;  // NEW: Required
        const NAME: &'static str = "MyClass";
    }
);
```

### Protocol Implementation

**objc2 0.6 Pattern:**
```rust
// 1. Declare the class
declare_class!(
    struct AppDelegate;

    unsafe impl ClassType for AppDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "AppDelegate";
    }
);

// 2. Implement protocol OUTSIDE declare_class!
unsafe impl NSApplicationDelegate for AppDelegate {}

// 3. Regular Rust impl block for Rust-side methods
impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send_id![mtm.alloc::<Self>(), init] }
    }
}
```

### ⚠️ What Doesn't Work

**DO NOT put method implementations inside `declare_class!`:**
```rust
// ❌ This will NOT compile in objc2 0.6
declare_class!(
    struct MyClass;

    unsafe impl ClassType for MyClass { ... }

    impl MyClass {  // ❌ ERROR: no rules expected `MyClass`
        #[method(myMethod)]
        fn my_method(&self) { ... }
    }
);
```

**The macro expects a specific structure and doesn't support custom impl blocks inside.**

---

## Method Calls

### msg_send! vs msg_send_id!

**For methods returning primitives or void:**
```rust
use objc2::msg_send;

let count: usize = unsafe { msg_send![obj, count] };
let _: () = unsafe { msg_send![obj, doSomething] };
```

**For methods returning objects:**
```rust
use objc2::msg_send_id;

let obj: Retained<NSString> = unsafe {
    msg_send_id![
        NSString::alloc(),
        initWithUTF8String: c"Hello".as_ptr()
    ]
};
```

### Window/View Creation Pattern

**Creating NSWindow (objc2 0.6):**
```rust
use objc2::msg_send;
use objc2_foundation::{MainThreadMarker, NSRect, NSPoint, NSSize};
use objc2_app_kit::{NSWindow, NSWindowStyleMask, NSBackingStoreType};

let mtm = MainThreadMarker::new().unwrap();

let frame = NSRect {
    origin: NSPoint { x: 0.0, y: 0.0 },
    size: NSSize { width: 800.0, height: 600.0 },
};

let style = NSWindowStyleMask::Titled
    | NSWindowStyleMask::Closable
    | NSWindowStyleMask::Resizable;

let window: Retained<NSWindow> = unsafe {
    msg_send![
        mtm.alloc::<NSWindow>(),
        initWithContentRect: frame,
        styleMask: style,
        backing: NSBackingStoreType::Buffered,
        defer: false
    ]
};
```

---

## Collections

### NSArray

**objc2 0.5:**
```rust
let array = NSArray::from_vec(vec![obj1, obj2, obj3]);
```

**objc2 0.6:**
```rust
// Takes slice of references now, not owned values
let array = NSArray::from_slice(&[&*obj1, &*obj2, &*obj3]);

// OR use as_super() for protocol types
let array = NSArray::from_slice(&[obj1.as_super(), obj2.as_super()]);
```

### Creating Arrays of Different Types

When you have different types that share a common supertype:

```rust
let input: Retained<VZVirtioSoundDeviceInputStreamConfiguration> = ...;
let output: Retained<VZVirtioSoundDeviceOutputStreamConfiguration> = ...;

// Downcast to common parent type
let input_stream: Retained<VZVirtioSoundDeviceStreamConfiguration> =
    input.downcast().unwrap();
let output_stream: Retained<VZVirtioSoundDeviceStreamConfiguration> =
    output.downcast().unwrap();

// Now create array with references
let streams = NSArray::from_slice(&[&*input_stream, &*output_stream]);
```

---

## Foundation Types

### Geometry Types

**objc2 0.5:** Often used `CGRect`, `CGPoint`, `CGSize`

**objc2 0.6:** Use `NS*` variants for AppKit

```rust
use objc2_foundation::{NSRect, NSPoint, NSSize};

let rect = NSRect {
    origin: NSPoint { x: 0.0, y: 0.0 },
    size: NSSize { width: 100.0, height: 100.0 },
};
```

### String Literals

**Creating NSString from Rust string:**
```rust
use objc2_foundation::NSString;

// From &str
let ns_str = NSString::from_str("Hello, World!");

// String literal macro
use objc2_foundation::ns_string;
let ns_str = ns_string!("Hello, World!");
```

### URLs

```rust
use objc2_foundation::{NSString, NSURL};
use std::path::PathBuf;

let path = PathBuf::from("/path/to/file");
let ns_path = NSString::from_str(&path.to_string_lossy());
let url = NSURL::fileURLWithPath(&ns_path);
```

### MainThreadMarker

**When to use:**
- Required for allocating AppKit/UIKit objects
- Ensures code runs on main thread
- Compile-time thread safety

```rust
use objc2_foundation::MainThreadMarker;

fn main() {
    let mtm = MainThreadMarker::new()
        .expect("Must run on main thread");

    let app = NSApplication::sharedApplication(mtm);
    // ... rest of app setup
}
```

**In callbacks:**
```rust
let callback = move || {
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    app.terminate(None);
};
```

---

## Common Patterns

### Pattern: Error Handling with Objective-C Methods

**Methods that return `Option<Retained<T>>`:**
```rust
let obj = unsafe { SomeClass::initWithSomething(...) }
    .ok_or_else(|| "Failed to initialize".to_string())?;
```

**Methods that return `Result<Retained<T>, Retained<NSError>>`:**
```rust
let obj = unsafe { SomeClass::initWithURL_error(...) }
    .map_err(|e| format!("Failed: {:?}", e))?;
```

**Methods that just return `Retained<T>` (never fail):**
```rust
let obj = unsafe { SomeClass::new() };  // No error handling needed
```

### Pattern: Keeping Objects Alive

**Problem:** Objects get deallocated when they go out of scope

**Solution 1: Box::leak (quick & dirty):**
```rust
// Keep alive for the entire program duration
Box::leak(Box::new(window));
Box::leak(Box::new(vm));
```

**Solution 2: Store in struct (proper):**
```rust
struct AppState {
    window: Retained<NSWindow>,
    vm: Retained<VZVirtualMachine>,
}

static APP_STATE: OnceCell<AppState> = OnceCell::new();
```

### Pattern: Working with Blocks

**Creating a block for callbacks:**
```rust
use block2::RcBlock;

let callback = RcBlock::new(|error: *mut NSError| {
    if !error.is_null() {
        eprintln!("Error: {:?}", unsafe { &*error });
    }
});

unsafe { obj.doSomethingWithCompletionHandler(&callback) };
```

### Pattern: Protocol Method Implementation

**You CANNOT implement protocol methods with bodies in objc2 0.6.**

The `declare_class!` macro is only for:
1. Declaring the class structure
2. Specifying class metadata (name, superclass, mutability)
3. Implementing protocol conformance (empty `unsafe impl Protocol for Class {}`)

For custom behavior, use regular Rust methods or function pointers.

---

## Troubleshooting

### Error: "no rules expected `ClassName`"

**Problem:**
```rust
declare_class!(
    struct MyClass;

    unsafe impl ClassType for MyClass { ... }

    impl MyClass {  // ❌ ERROR HERE
        ...
    }
);
```

**Solution:** Move `impl` block outside `declare_class!`:
```rust
declare_class!(
    struct MyClass;

    unsafe impl ClassType for MyClass { ... }
);

impl MyClass {  // ✅ Outside the macro
    ...
}
```

### Error: "function or associated item `alloc` exists for struct X, but its trait bounds were not satisfied"

**Problem:** `ClassType` trait not in scope

**Solution:**
```rust
use objc2::ClassType;  // Import the trait

let obj = unsafe { MyClass::alloc() };
```

**OR use MainThreadMarker for AppKit classes:**
```rust
let mtm = MainThreadMarker::new().unwrap();
let obj = mtm.alloc::<NSWindow>();
```

### Error: "no function or associated item named `from_vec`"

**Problem:** `NSArray::from_vec()` was removed

**Solution:**
```rust
// Old (0.5)
let array = NSArray::from_vec(vec![obj1, obj2]);

// New (0.6)
let array = NSArray::from_slice(&[&*obj1, &*obj2]);
```

### Error: "cannot find struct, variant or union type `CGRect`"

**Problem:** Using Core Graphics types instead of Foundation types

**Solution:**
```rust
// Wrong
use objc2_foundation::{CGRect, CGPoint, CGSize};

// Correct for AppKit
use objc2_foundation::{NSRect, NSPoint, NSSize};
```

### Error: "method `map_err` not found for struct `Retained<T>`"

**Problem:** Method doesn't return `Result`, it returns `Retained<T>` directly

**Solution:**
```rust
// If method signature is: fn foo() -> Retained<T>
let obj = unsafe { SomeClass::foo() };  // Just use it directly

// If method signature is: fn foo() -> Option<Retained<T>>
let obj = unsafe { SomeClass::foo() }
    .ok_or_else(|| "Error".to_string())?;
```

### Error: "unexpected end of macro invocation" in declare_class!

**Problem:** Missing required sections in `declare_class!`

**Solution:** Ensure you have all required parts:
```rust
declare_class!(
    struct MyClass;  // Semicolon required

    unsafe impl ClassType for MyClass {  // Required
        type Super = NSObject;
        type Mutability = MainThreadOnly;  // Required in 0.6
        const NAME: &'static str = "MyClass";
    }
    // That's it! Nothing else goes inside.
);
```

---

## Quick Reference Card

| Task | objc2 0.5 | objc2 0.6 |
|------|-----------|-----------|
| Object type | `Id<T>` | `Retained<T>` |
| Allocate object | `T::alloc()` | `T::alloc()` (trait in scope) or `mtm.alloc::<T>()` |
| Create NSArray | `NSArray::from_vec(vec![...])` | `NSArray::from_slice(&[&*obj, ...])` |
| Geometry types | `CGRect/CGPoint/CGSize` | `NSRect/NSPoint/NSSize` (AppKit) |
| Cast to super | `Retained::cast(obj)` | `obj.downcast()` or `obj.as_super()` |
| msg_send returns object | `msg_send_id!` | `msg_send_id!` (unchanged) |
| Class mutability | Not specified | `type Mutability = MainThreadOnly` |

---

## Best Practices

1. **Always import `ClassType` when using `::alloc()`**
   ```rust
   use objc2::ClassType;
   ```

2. **Use `MainThreadMarker` for AppKit/UIKit**
   ```rust
   let mtm = MainThreadMarker::new().unwrap();
   let obj = mtm.alloc::<NSWindow>();
   ```

3. **Keep protocol impls outside `declare_class!`**
   ```rust
   declare_class!( ... );
   unsafe impl SomeProtocol for MyClass {}
   ```

4. **Prefer safe wrappers when available**
   ```rust
   // Instead of unsafe msg_send
   app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
   ```

5. **Use `from_slice` with references for NSArray**
   ```rust
   NSArray::from_slice(&[&*obj1, &*obj2])
   ```

6. **Check method signatures in auto-generated bindings**
   - Does it return `Retained<T>`, `Option<Retained<T>>`, or `Result<...>`?
   - Adjust error handling accordingly

---

## Resources

- **objc2 docs:** https://docs.rs/objc2/
- **objc2-foundation:** https://docs.rs/objc2-foundation/
- **objc2-app-kit:** https://docs.rs/objc2-app-kit/
- **objc2-virtualization:** https://docs.rs/objc2-virtualization/

---

## Summary

The migration from objc2 0.5 to 0.6 involves:
- Changing `Id<T>` to `Retained<T>`
- Adding `type Mutability` to class declarations
- Using `from_slice` instead of `from_vec` for NSArray
- Importing `ClassType` trait explicitly
- Keeping protocol implementations outside `declare_class!`
- Using `NS*` geometry types for AppKit
- Leveraging `MainThreadMarker` for thread-safe main-thread allocation

The auto-generated bindings make the API more consistent and easier to use, but require understanding the new patterns and conventions.
