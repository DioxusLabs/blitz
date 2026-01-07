# objc2 Rust Guide for macOS Framework Bindings

This guide documents patterns and best practices for using `objc2` and related crates (`objc2-foundation`, `objc2-app-kit`, `objc2-virtualization`, etc.) to interface with Apple's Objective-C frameworks from Rust.

## Table of Contents
1. [Strategy & Workflow](#strategy--workflow) ⭐ **READ THIS FIRST**
2. [Crate Structure](#crate-structure)
3. [Key Types](#key-types)
4. [Allocation and Initialization](#allocation-and-initialization)
5. [Class Inheritance and Type Coercion](#class-inheritance-and-type-coercion)
6. [Defining Custom Objective-C Classes](#defining-custom-objective-c-classes)
7. [Working with NSArray](#working-with-nsarray)
8. [Delegates and Protocols](#delegates-and-protocols)
9. [Main Thread Safety](#main-thread-safety)
10. [Memory Management](#memory-management)
11. [Common Patterns](#common-patterns)
12. [Troubleshooting](#troubleshooting)

---

## Strategy & Workflow

**This section captures the optimal workflow for writing objc2 code. Follow this process to minimize iteration cycles with the compiler.**

### Phase 1: Research Before Coding

1. **Check crate features FIRST** before adding to Cargo.toml:
   ```bash
   cargo search objc2-av-foundation
   # Then check available features:
   curl -s https://crates.io/api/v1/crates/objc2-av-foundation/0.3.2 | \
     python3 -c "import sys,json; [print(k) for k in sorted(json.load(sys.stdin)['version']['features'].keys())]"
   ```

2. **Feature naming patterns**:
   - Features usually match class names: `AVCaptureSession`, `VZVirtualMachine`
   - Subclasses are often bundled with parent: `AVCaptureDeviceInput` is part of `AVCaptureInput`
   - When in doubt, search for the parent class name

3. **Use documentation lookup tools** to understand available types and method signatures before writing code.

### Phase 2: Understand Naming Conventions

The objc2 crates follow consistent naming transformations from Objective-C:

| Objective-C | Rust |
|------------|------|
| `kCALayerWidthSizable` | `CAAutoresizingMask::LayerWidthSizable` |
| `NSBackingStoreBuffered` | `NSBackingStoreType::Buffered` |
| `AVMediaTypeVideo` | `AVMediaTypeVideo` (extern static, needs unsafe) |
| `-initWithFoo:bar:` | `initWithFoo_bar(alloc, ...)` |

**Key pattern**: Constants drop their prefix (`kCA`, `NS`) and become associated constants on a type.

### Phase 3: Handle Extern Statics Correctly

Global constants like `AVMediaTypeVideo` are extern statics:
```rust
// These are Option<&'static NSString> and need unsafe access
let media_type = unsafe { AVMediaTypeVideo.expect("not available") };
```

### Phase 4: Build Incrementally

1. **Run `cargo check` early** - after imports, after each function
2. **Don't write the entire file** before checking if it compiles
3. **Trust compiler errors** for API details you couldn't verify upfront
4. **Check if unsafe is actually needed** - many methods are safe with MainThreadMarker

### Phase 5: Common Gotchas Checklist

Before your first build, verify:

- [ ] Imported `AnyThread` trait if using `Type::alloc()` on non-UI types
- [ ] Using `into_super()` on array *elements*, not the array itself
- [ ] `define_class!` struct has semicolon (no inline fields)
- [ ] Thread-local storage set up for state that must stay alive
- [ ] Checked if methods actually need `unsafe` (many are safe)

### Anti-Patterns to Avoid

```rust
// DON'T: Wrap everything in unsafe "just in case"
unsafe { app.run() };  // run() is actually safe!

// DON'T: Guess constant names from Objective-C
objc2_quartz_core::kCALayerWidthSizable  // Wrong!

// DON'T: Assume extern statics are non-optional
AVMediaTypeVideo  // This is Option<&NSString>, not &NSString

// DON'T: Write the whole file before checking compilation
// DO: Build incrementally, let compiler guide you
```

### Debugging Strategy

When the build fails:
1. Read the error message carefully - rustc is usually very helpful
2. Check the suggested type vs expected type
3. Look for `into_super()` issues with NSArray
4. Check if you need `unsafe` or are using it unnecessarily
5. Verify feature flags are enabled for the types you're using

---

## Crate Structure

The objc2 ecosystem is organized as follows:

```toml
[dependencies]
# Core runtime
objc2 = "0.6"
block2 = "0.6"  # For Objective-C blocks (callbacks)

# Foundation types (NSString, NSArray, NSURL, etc.)
objc2-foundation = { version = "0.3", features = ["NSString", "NSArray", ...] }

# AppKit for macOS GUI
objc2-app-kit = { version = "0.3", features = ["NSApplication", "NSWindow", ...] }

# Framework-specific crates
objc2-virtualization = { version = "0.3", features = ["VZVirtualMachine", ...] }
```

**Important**: Each type/class requires its feature to be enabled explicitly.

---

## Key Types

### `Retained<T>`
Smart pointer for Objective-C objects with automatic reference counting (ARC).

```rust
use objc2::rc::Retained;

let obj: Retained<NSString> = NSString::from_str("hello");

// Dereference to get &T
let reference: &NSString = &*obj;
// Or use Deref coercion
some_method(&obj);  // If method takes &NSString
```

### `MainThreadMarker`
Proof that code is running on the main thread. Required for UI operations.

```rust
use objc2_foundation::MainThreadMarker;

fn main() {
    let mtm = MainThreadMarker::new().expect("Must run on main thread");

    // Use mtm to allocate main-thread-only objects
    let window = unsafe { NSWindow::initWith...(mtm.alloc(), ...) };
}
```

### `ProtocolObject<dyn SomeProtocol>`
Type-erased protocol object for delegate patterns.

```rust
use objc2::runtime::ProtocolObject;

let delegate = MyDelegate::new(mtm);
app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
```

---

## Allocation and Initialization

### For `MainThreadOnly` types (UI classes)
Use `mtm.alloc::<T>()`:

```rust
let mtm = MainThreadMarker::new().unwrap();

// Allocate main-thread-only types
let window = unsafe {
    NSWindow::initWithContentRect_styleMask_backing_defer(
        mtm.alloc::<NSWindow>(),
        rect,
        style_mask,
        backing_store,
        false,
    )
};
```

### For `AnyThread` types (non-UI classes)
Use `T::alloc()` with `AnyThread` trait in scope:

```rust
use objc2::AnyThread;

let config = unsafe {
    VZMacGraphicsDisplayConfiguration::initWithWidthInPixels_heightInPixels_pixelsPerInch(
        VZMacGraphicsDisplayConfiguration::alloc(),
        1920,
        1200,
        80,
    )
};
```

### Using `new()` (when available)
Many types provide a `new()` method that combines alloc+init:

```rust
let boot_loader = unsafe { VZMacOSBootLoader::new() };
let config = unsafe { VZVirtualMachineConfiguration::new() };
```

---

## Class Inheritance and Type Coercion

### The Problem
Objective-C uses class inheritance, but Rust's type system is strict. When a method expects `&NSArray<BaseClass>` but you have `Retained<SubClass>`, you need to coerce.

### Solution: `into_super()`
Use `.into_super()` to convert a `Retained<SubClass>` into `Retained<SuperClass>`:

```rust
// VZMacGraphicsDeviceConfiguration inherits from VZGraphicsDeviceConfiguration
let graphics: Retained<VZMacGraphicsDeviceConfiguration> = create_graphics_config();

// Convert to parent type for array that expects the base class
let graphics_base: Retained<VZGraphicsDeviceConfiguration> = graphics.into_super();

// Now it works with NSArray<VZGraphicsDeviceConfiguration>
let array = NSArray::from_retained_slice(&[graphics_base]);
unsafe { config.setGraphicsDevices(&array) };
```

### Common inheritance chains:
```
VZMacGraphicsDeviceConfiguration -> VZGraphicsDeviceConfiguration
VZVirtioBlockDeviceConfiguration -> VZStorageDeviceConfiguration
VZVirtioNetworkDeviceConfiguration -> VZNetworkDeviceConfiguration
VZMacKeyboardConfiguration -> VZKeyboardConfiguration
VZMacTrackpadConfiguration -> VZPointingDeviceConfiguration
VZMacPlatformConfiguration -> VZPlatformConfiguration
```

---

## Defining Custom Objective-C Classes

Use `define_class!` macro for custom classes (delegates, subclasses):

```rust
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};

define_class!(
    // Specify parent class
    #[unsafe(super = NSObject)]

    // Thread safety (use MainThreadOnly for UI-related classes)
    #[thread_kind = MainThreadOnly]

    // Objective-C class name
    #[name = "MyDelegate"]

    // Struct definition (NO inline ivars in objc2 0.6.x!)
    struct MyDelegate;

    // Implement NSObjectProtocol (required)
    unsafe impl NSObjectProtocol for MyDelegate {}

    // Implement any protocols
    unsafe impl SomeDelegate for MyDelegate {
        #[unsafe(method(someMethod:))]
        fn some_method(&self, arg: &SomeType) {
            // Implementation
        }
    }
);

impl MyDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![mtm.alloc::<Self>(), init] }
    }
}
```

### Important: No inline ivars in objc2 0.6.x
The `define_class!` macro does NOT support inline instance variables:

```rust
// WRONG - won't compile in objc2 0.6.x
struct MyClass {
    field: SomeType,  // NO!
}

// CORRECT - use semicolon, no fields
struct MyClass;
```

**Workaround for state**: Use thread-local storage:

```rust
use std::cell::RefCell;

thread_local! {
    static STATE: RefCell<Option<MyState>> = const { RefCell::new(None) };
}

struct MyState {
    window: Retained<NSWindow>,
    // other fields...
}

// Store state when needed
STATE.with(|s| {
    *s.borrow_mut() = Some(MyState { window, ... });
});
```

---

## Working with NSArray

### Creating arrays
```rust
use objc2_foundation::NSArray;

// From a slice of Retained objects
let items: Vec<Retained<SomeType>> = vec![item1, item2];
let array = NSArray::from_retained_slice(&items);

// Or inline
let array = NSArray::from_retained_slice(&[item1, item2]);
```

### Type coercion for arrays
When the setter expects a different (parent) type:

```rust
// Method expects &NSArray<VZGraphicsDeviceConfiguration>
// But we have Retained<VZMacGraphicsDeviceConfiguration>

let mac_graphics = create_mac_graphics_config();
let base_graphics = mac_graphics.into_super();  // Coerce to parent type
let array = NSArray::from_retained_slice(&[base_graphics]);
unsafe { config.setGraphicsDevices(&array) };
```

---

## Delegates and Protocols

### Setting a delegate
```rust
use objc2::runtime::ProtocolObject;

let delegate = MyDelegate::new(mtm);

// Convert to protocol object for setDelegate
unsafe {
    vm.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
}

// IMPORTANT: Keep delegate alive! Store it somewhere.
```

### Implementing delegate methods
```rust
define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "VMDelegate"]
    struct VMDelegate;

    unsafe impl NSObjectProtocol for VMDelegate {}

    unsafe impl VZVirtualMachineDelegate for VMDelegate {
        #[unsafe(method(guestDidStopVirtualMachine:))]
        fn guest_did_stop(&self, _vm: &VZVirtualMachine) {
            println!("VM stopped");
            // Note: Can access MainThreadMarker here since we're MainThreadOnly
            let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            app.terminate(None);
        }

        #[unsafe(method(virtualMachine:didStopWithError:))]
        fn vm_did_stop_with_error(&self, _vm: &VZVirtualMachine, error: &NSError) {
            eprintln!("VM error: {:?}", error);
        }
    }
);
```

---

## Main Thread Safety

### MainThreadOnly types
Types marked with `MainThreadOnly` can only be created/used on the main thread:
- `NSApplication`, `NSWindow`, `NSView`, `VZVirtualMachineView`
- Custom classes with `#[thread_kind = MainThreadOnly]`

### Obtaining MainThreadMarker
```rust
// At program start
let mtm = MainThreadMarker::new().expect("Must be on main thread");

// In delegate methods (if class is MainThreadOnly)
let mtm = MainThreadMarker::new().unwrap();  // Safe because we know we're on main thread
```

### Methods that don't need unsafe
Many methods on `MainThreadOnly` types are safe when you have proof of main thread:
```rust
// These are safe (no unsafe needed)
let app = NSApplication::sharedApplication(mtm);
app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
app.run();

window.setTitle(ns_string!("Title"));
window.center();
window.makeKeyAndOrderFront(None);
```

---

## Memory Management

### Retained keeps objects alive
```rust
let delegate = MyDelegate::new(mtm);
app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

// delegate must stay alive! Don't let it drop.
// Store it in thread-local, struct field, or keep in scope.
```

### Thread-local storage pattern
```rust
thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    _window: Retained<NSWindow>,      // Keep window alive
    _delegate: Retained<MyDelegate>,  // Keep delegate alive
}

// Store after creation
APP_STATE.with(|state| {
    *state.borrow_mut() = Some(AppState {
        _window: window,
        _delegate: delegate,
    });
});
```

---

## Common Patterns

### Creating an AppKit application
```rust
fn main() {
    let mtm = MainThreadMarker::new().expect("Must run on main thread");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();  // Blocks until app terminates
}
```

### Completion handlers with blocks
```rust
use block2::RcBlock;

let completion = RcBlock::new(|error: *mut NSError| {
    if !error.is_null() {
        let err = unsafe { &*error };
        eprintln!("Error: {:?}", err);
    } else {
        println!("Success!");
    }
});

unsafe {
    some_async_method(&completion);
}
```

### Working with NSString
```rust
use objc2_foundation::{NSString, ns_string};

// From &str
let string = NSString::from_str("hello");

// Static string (compile-time)
window.setTitle(ns_string!("Window Title"));
```

### Working with NSURL
```rust
use objc2_foundation::NSURL;

let path = "/path/to/file";
let url = NSURL::fileURLWithPath(&NSString::from_str(path));
```

### Working with NSData
```rust
use objc2_foundation::NSData;

let bytes: Vec<u8> = fs::read("file.bin")?;
let data = NSData::with_bytes(&bytes);
```

---

## Critical Checklist (Read Before Writing Code!)

Before writing objc2 code, verify these common pitfalls:

### 1. Import `AnyThread` for non-UI allocations
If you're calling `.alloc()` on any non-MainThreadOnly type, you MUST import the trait:
```rust
use objc2::AnyThread;  // Required for Type::alloc() to work!
```
Without this, you'll get "no function or associated item named `alloc` found".

### 2. `into_super()` goes on ELEMENTS, not arrays
When building an NSArray for a setter that expects a parent type:
```rust
// WRONG - calling into_super() on the array
let array = NSArray::from_retained_slice(&[item]);
config.setDevices(&array.into_super());  // NO!

// CORRECT - call into_super() on each element BEFORE the array
let array = NSArray::from_retained_slice(&[item.into_super()]);
config.setDevices(&array);  // YES!
```

### 3. No inline ivars in define_class! (objc2 0.6.x)
```rust
// WRONG - will fail with "no rules expected this token"
define_class!(
    #[unsafe(super(NSObject))]
    struct MyClass {
        field: Type,  // NO!
    }
);

// CORRECT - empty struct, use thread-local for state
define_class!(
    #[unsafe(super(NSObject))]
    struct MyClass;  // Semicolon, no body!
);
```

### 4. Always use parentheses in #[unsafe(super(...))]
```rust
// CORRECT syntax
#[unsafe(super(NSObject))]

// NOT this (older syntax)
#[unsafe(super = NSObject)]
```

### 5. Retain objects that must stay alive
Delegates, windows, and other objects passed to Objective-C must be kept alive:
```rust
thread_local! {
    static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

// Store BEFORE the function returns
STATE.with(|s| *s.borrow_mut() = Some(AppState { delegate, window, ... }));
```

### 6. Only import the types you actually use
Don't import parent classes if you only use subclasses:
```rust
// If you only create VZMacGraphicsDeviceConfiguration,
// you don't need to import VZGraphicsDeviceConfiguration
// (into_super() handles the conversion)
```

### 7. Check method safety
Some methods are safe with MainThreadMarker proof, others always need unsafe:
```rust
// Safe (takes mtm as proof)
let app = NSApplication::sharedApplication(mtm);
app.setActivationPolicy(...);  // Safe
app.run();  // Safe

// Unsafe (framework-specific operations)
unsafe { config.setBootLoader(...) };
unsafe { vm.startWithCompletionHandler(...) };
```

---

## Troubleshooting

### "no rules expected this token" in define_class!
You're trying to use inline ivars. Remove them:
```rust
// Wrong
struct MyClass { field: Type }

// Correct
struct MyClass;
```

### "no function or associated item named `alloc` found"
Import the `AnyThread` trait:
```rust
use objc2::AnyThread;  // For non-MainThreadOnly types
// MainThreadOnly types use mtm.alloc::<T>()
```

### "mismatched types" with NSArray
Use `.into_super()` to coerce subclass to parent class:
```rust
let array = NSArray::from_retained_slice(&[subclass_item.into_super()]);
```

### "expected &T, found &Retained<T>"
Dereference with `&*`:
```rust
// Wrong
method(&retained_obj);

// Correct (if method takes &T)
method(&*retained_obj);
// Or let Deref coercion work
method(&retained_obj);  // Sometimes works
```

### Delegate/callback not being called
Make sure the delegate object stays alive:
```rust
// Store in thread-local or struct field
let delegate = MyDelegate::new(mtm);
app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
// delegate MUST NOT be dropped here!
```

### NSBackingStoreType variants
Use `NSBackingStoreType::Buffered` (not `NSBackingStoreBuffered`).

---

## Virtualization Framework Specifics

### Required entitlements
Create `entitlements.plist`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "...">
<plist version="1.0">
<dict>
    <key>com.apple.security.virtualization</key>
    <true/>
</dict>
</plist>
```

Sign binary:
```bash
codesign --entitlements entitlements.plist --force -s - target/release/myapp
```

### VM Bundle structure
```
~/VM.bundle/
├── AuxiliaryStorage    # Boot data
├── Disk.img            # Virtual disk
├── HardwareModel       # Hardware model data
├── MachineIdentifier   # Machine ID data
└── SaveFile.vzvmsave   # Optional saved state
```

### Loading persisted hardware identity
```rust
fn load_hardware_model(path: &Path) -> Retained<VZMacHardwareModel> {
    let data = fs::read(path).expect("Failed to read");
    let ns_data = NSData::with_bytes(&data);
    unsafe {
        VZMacHardwareModel::initWithDataRepresentation(
            VZMacHardwareModel::alloc(),
            &ns_data,
        ).expect("Invalid hardware model")
    }
}
```
