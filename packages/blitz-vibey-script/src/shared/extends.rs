//! The `Extended<T>` scheme: real `super` hooks — calling `sup` triggers the
//! parent's recursive construction.
//!
//! - `T: ExtendLayer` is not a Boa class, just a native data container
//!   describing one layer of an inheritance hierarchy.
//! - `Extended<T>` itself implements `Class`. The instance's native data slot
//!   holds a single `OwnDataRegistry`: one fixed-size slot per real layer in
//!   the chain, addressed by the compile-time `OwnBlock::IDX`. There are no
//!   Symbols and no per-layer JS objects; reads and writes are pure Rust-side
//!   slot borrows + `TypeId` downcasts.
//! - `build(args, ctx, sup)`: the subclass calls `sup.call(parent_args, ctx)`
//!   at the point where ES semantics place `super`. At that moment the
//!   parent's `emit_own` actually runs on the instance and fills the parent's
//!   slot; code after the call reads the parent's data via the `SuperDone`
//!   token.
//! - "Forgetting to call super" is enforced by the type system: `build` must
//!   return `Constructed<Self>`, which can only be assembled from the
//!   `SuperDone` produced by `sup.call(...)`.
//! - Instantiation entry points:
//!   - JS `new X()`: `Class::construct` attaches the registry, recursive
//!     `emit_own` fills each layer's slot from the args.
//!   - Rust (args-buildable layers): `Extended::new_native(args, ctx)`.
//!   - Rust (existing data chain): `Extended::from_chain(chain, ctx)`; the
//!     `LayerChain` fields are non-`Option` and are moved into place by
//!     recursive `populate_chain`.
//! - Own blocks are read and written through `with_own` (zero-copy read) and
//!   `with_own_mut` (in-place write). While an `OwnDataRegistry` is borrowed
//!   through `with_own*`, the same instance must not be downcast again
//!   (the registry borrow is held for the duration of the callback).

use boa_engine::class::{Class, ClassBuilder};
use boa_engine::gc::GcRefCell;
use boa_engine::object::{NativeObject, PROTOTYPE};
use boa_engine::{Context, Finalize, JsData, JsObject, JsResult, JsValue, Trace};
use std::any::Any;
use std::marker::PhantomData;

// ── OwnBlock: compile-time slot layout ───────────────────────────────

/// Compile-time layout of a layer's own data inside the per-instance
/// `OwnDataRegistry`. `RootLayer` is the chain terminator with `DEPTH = 0`;
/// every `ExtendLayer` derives `Parent::DEPTH + 1`. `DEPTH` is the number of
/// real layers on the chain (so it is also the exact registry size), and
/// `IDX` is the layer's slot position, `DEPTH - 1`.
pub trait OwnBlock: OwnSlot + 'static {
    /// Number of real layers from the chain root down to and including
    /// `Self`. `RootLayer::DEPTH = 0` — it occupies no slot.
    const DEPTH: usize;

    /// The slot position of `Self`'s own data: `DEPTH - 1`.
    const IDX: usize = Self::DEPTH - 1;
}

impl OwnBlock for RootLayer {
    const DEPTH: usize = 0;
}

impl<T: ExtendLayer> OwnBlock for T {
    const DEPTH: usize = <T::Parent as OwnBlock>::DEPTH + 1;
}

// ── OwnDataRegistry: sized per-instance slots ────────────────────────

/// Type-erased own-block slot: `Any` for downcasting, `Trace` so the GC can
/// reach every `JsValue` the block owns. The `Any` bound lives on the
/// blanket impl, not the trait: `dyn OwnSlot` must implement `OwnSlot` so
/// accessors can be reached through it.
pub trait OwnSlot: Trace {
    fn as_any_ref(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Trace> OwnSlot for T {
    fn as_any_ref(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The per-instance own-data store. One slot per real layer in the chain
/// (`RootLayer` takes none); `T::IDX` addresses a layer's slot directly, so
/// there is no bounds check or growth logic — the slot list is sized to the
/// leaf layer's `DEPTH` at attach time.
///
/// Each slot is its own `GcRefCell`, so borrowing layer A's slot never blocks
/// layer B's — only concurrent shared/mutable access to the same layer's slot
/// conflicts.
#[derive(Trace, Finalize, JsData)]
pub struct OwnDataRegistry {
    slots: Vec<GcRefCell<Option<Box<dyn OwnSlot>>>>,
}

impl OwnDataRegistry {
    /// Build a registry sized for a chain ending at `T` (the leaf layer).
    /// `T::DEPTH` is a compile-time constant, so the slots are allocated in
    /// one shot. Slots start empty; `set_own_block` fills them layer by layer
    /// during construction.
    fn new<T: OwnBlock>() -> Self {
        Self {
            slots: (0..T::DEPTH)
                .map(|_| GcRefCell::new(None))
                .collect(),
        }
    }

    /// Write `T`'s data into its slot. `T::IDX` is a compile-time constant
    /// and a registry is always sized for the chain `T` is constructed on,
    /// so this is a plain slot assignment with no bounds check.
    #[inline]
    fn set_slot_for<T: ExtendLayer>(&self, data: T) {
        *self.slots[T::IDX].borrow_mut() = Some(Box::new(data));
    }

    /// Resolve the slot cell for `T`. Fails if `T::IDX` is out of range (the
    /// receiver was built for a shorter chain — e.g. a child layer's getter
    /// invoked on a parent-only instance).
    #[inline]
    fn slot_for<T: ExtendLayer>(&self) -> JsResult<&GcRefCell<Option<Box<dyn OwnSlot>>>> {
        self.slots.get(T::IDX).ok_or_else(|| {
            crate::shared::native_error!(
                typ,
                "{}: own slot index {} out of range",
                T::CLASS_NAME,
                T::IDX
            )
        })
    }

    fn with<T: ExtendLayer, R>(&self, f: impl FnOnce(&T) -> R) -> JsResult<R> {
        let slot = self.slot_for::<T>()?.borrow();
        // `as_deref` goes straight to `&dyn OwnSlot`; taking `&Box<dyn
        // OwnSlot>` would resolve `as_any_ref` to the blanket impl on
        // `Box<dyn OwnSlot>` itself instead of the inner layer.
        let Some(data) = slot.as_deref() else {
            return Err(crate::shared::native_error!(
                typ,
                "{}: own block not constructed",
                T::CLASS_NAME
            ));
        };
        let data = data
            .as_any_ref()
            .downcast_ref::<T>()
            .ok_or_else(|| {
                let msg = format!(
                    "{}: own block type mismatch (expected any={:?}, actual any={:?})",
                    T::CLASS_NAME,
                    std::any::TypeId::of::<T>(),
                    data.as_any_ref().type_id()
                );
                boa_engine::JsError::from(boa_engine::JsNativeError::typ().with_message(msg))
            })?;
        Ok(f(data))
    }

    fn with_mut<T: ExtendLayer, R>(&self, f: impl FnOnce(&mut T) -> R) -> JsResult<R> {
        let mut slot = self.slot_for::<T>()?.borrow_mut();
        let Some(data) = slot.as_deref_mut() else {
            return Err(crate::shared::native_error!(
                typ,
                "{}: own block not constructed",
                T::CLASS_NAME
            ));
        };
        let data = data
            .as_any_mut()
            .downcast_mut::<T>()
            .ok_or_else(|| crate::shared::native_error!(typ, "{}: own block type mismatch", T::CLASS_NAME))?;
        Ok(f(data))
    }
}

// ── Registry attach + accessor helpers ───────────────────────────────

/// Write a layer's data into its slot.
#[inline]
pub fn set_own_block<T: ExtendLayer>(this: &JsObject, data: T) -> JsResult<()> {
    let registry = this
        .downcast_ref::<OwnDataRegistry>()
        .ok_or_else(|| crate::shared::native_error!(typ, "instance has no own data registry"))?;
    registry.set_slot_for::<T>(data);
    Ok(())
}

/// Read a layer's own block by shared reference. The registry borrow is held
/// for the duration of `f` — do not downcast `obj` again before it returns.
#[inline]
pub fn with_own<T, R>(obj: &JsObject, f: impl FnOnce(&T) -> R) -> JsResult<R>
where
    T: ExtendLayer,
{
    let registry = obj
        .downcast_ref::<OwnDataRegistry>()
        .ok_or_else(|| crate::shared::native_error!(typ, "instance has no own data registry"))?;
    registry.with::<T, R>(f)
}

/// Mutate a layer's own block in place through the callback. The `&mut T`
/// borrow lives only inside `f` — do not touch the same own block again
/// before it returns.
#[inline]
pub fn with_own_mut<T, R>(obj: &JsObject, f: impl FnOnce(&mut T) -> R) -> JsResult<R>
where
    T: ExtendLayer,
{
    let registry = obj
        .downcast_ref::<OwnDataRegistry>()
        .ok_or_else(|| crate::shared::native_error!(typ, "instance has no own data registry"))?;
    registry.with_mut::<T, R>(f)
}

// ── Super handle + Constructed token ─────────────────────────────────

/// Receipt proving that `super` has run. Only `Super::call` can produce one.
/// Carries the instance: after super, the subclass reads the parent's data
/// through it, matching ES "this is only usable after super". The field is
/// private so `SuperDone` cannot be forged — the type system truly enforces
/// super-before-return.
pub struct SuperDone<'a> {
    this: &'a JsObject,
}

impl<'a> SuperDone<'a> {
    /// The instance, valid once every parent layer's slot is filled.
    pub fn this(&self) -> &JsObject {
        self.this
    }
}

/// `build`'s return value. Assemblable only from a `SuperDone` + own data,
/// which enforces super-before-return.
pub struct Constructed<T> {
    own: T,
}

impl<T> Constructed<T> {
    /// Assemble from the super-completion token + the layer's own data.
    pub fn new(_done: SuperDone<'_>, own: T) -> Self {
        Constructed { own }
    }
}

/// The super handle: holds the instance; `call` triggers the parent's
/// `P::emit_own` recursive construction.
pub struct Super<'a, P: EmitOwn> {
    instance: &'a JsObject,
    _parent: PhantomData<fn() -> P>,
}

impl<'a, P: EmitOwn> Super<'a, P> {
    /// Trigger the parent's recursive construction (fills the parent's slot
    /// on the instance) and return the completion token carrying the
    /// instance. After this call the parent's data is fully initialized and
    /// readable through the token.
    #[inline]
    pub fn call(self, parent_args: &[JsValue], ctx: &mut Context) -> JsResult<SuperDone<'a>> {
        P::emit_own(self.instance, parent_args, ctx)?;
        Ok(SuperDone { this: self.instance })
    }
}

// ── ExtendLayer trait: describes one data layer ──────────────────────

pub trait ExtendLayer: OwnBlock + Sized + NativeObject + Clone + 'static {
    type Parent: EmitOwn + OwnBlock + 'static;
    const CLASS_NAME: &'static str;

    /// Build this layer. Must call `sup.call(parent_args, ctx)` where ES
    /// semantics place `super`, and assemble the returned `SuperDone` into
    /// the `Constructed`. Code order before/after the call is the ES order.
    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>>;

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()>;
}

// ── EmitOwn: recursive driver ────────────────────────────────────────

pub trait EmitOwn {
    /// JS `new` path: build each layer from the args and fill its slot on
    /// the instance.
    fn emit_own(instance: &JsObject, args: &[JsValue], ctx: &mut Context) -> JsResult<()>;

    /// The chain type of the Rust data path. `RootLayer::Chain` is `()`;
    /// `T::Chain` is `LayerChain<T>`.
    type Chain;

    /// Fill the instance's slots from an existing data chain (move
    /// destructure, no take), parent layer first.
    fn populate_chain(instance: &JsObject, chain: Self::Chain, ctx: &mut Context) -> JsResult<()>;
}

/// Chain terminator: no parent, no own data.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct RootLayer;

impl EmitOwn for RootLayer {
    type Chain = ();

    fn emit_own(_instance: &JsObject, _args: &[JsValue], _ctx: &mut Context) -> JsResult<()> {
        Ok(())
    }

    fn populate_chain(_instance: &JsObject, _chain: (), _ctx: &mut Context) -> JsResult<()> {
        Ok(())
    }
}

/// A node of the Rust data chain: this layer's own data plus the parent
/// chain. Fields are non-`Option` and move straight into the slots.
pub struct LayerChain<T: ExtendLayer> {
    pub parent: <T::Parent as EmitOwn>::Chain,
    pub own: T,
}

impl<T: ExtendLayer> EmitOwn for T {
    type Chain = LayerChain<T>;

    #[inline]
    fn emit_own(instance: &JsObject, args: &[JsValue], ctx: &mut Context) -> JsResult<()> {
        // Hand the super handle to build; the parent's construction is
        // triggered by the `sup.call` inside build.
        let sup = Super::<T::Parent> { instance, _parent: PhantomData };
        let constructed = T::build(args, ctx, sup)?;
        // By the time build returns, super has run (guaranteed by SuperDone)
        // and this layer's own data is written into its slot.
        set_own_block::<T>(instance, constructed.own)
    }

    #[inline]
    fn populate_chain(instance: &JsObject, chain: LayerChain<T>, ctx: &mut Context) -> JsResult<()> {
        // Parent slots first (ES super order): move the field in directly.
        T::Parent::populate_chain(instance, chain.parent, ctx)?;
        set_own_block::<T>(instance, chain.own)
    }
}

// ── Extended<T> + Class impl (data slot holds the registry) ──────────

/// The `Class` native data used only on the default `data_constructor` path.
/// Real instances built through `construct` carry an `OwnDataRegistry`
/// instead; all layer data lives in its slots.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct Extended<T: ExtendLayer> {
    _marker: PhantomData<fn() -> T>,
}

impl<T: ExtendLayer> Extended<T> {
    /// ZST shell returned by `data_constructor` (never instantiated on the
    /// overridden `construct` path).
    fn shell() -> Self {
        Extended { _marker: PhantomData }
    }
}

impl<T> Class for Extended<T>
where
    T: ExtendLayer + 'static,
{
    const NAME: &'static str = T::CLASS_NAME;

    /// Default data path; the overridden `construct` never runs it.
    fn data_constructor(_nt: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> JsResult<Self> {
        Ok(Extended::shell())
    }

    /// JS `new X()` path: resolve the prototype, attach an `OwnDataRegistry`
    /// sized for this chain, and fill the layers' slots recursively.
    #[inline]
    fn construct(new_target: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsObject> {
        if new_target.is_undefined() {
            return Err(crate::shared::native_error!(
                typ,
                "cannot construct `{}` without new",
                T::CLASS_NAME
            ));
        }
        let prototype = match new_target
            .as_object()
            .and_then(|c| c.get(PROTOTYPE, ctx).ok())
            .and_then(|p| p.as_object())
        {
            Some(p) => p,
            None => Self::registered_prototype(ctx)?,
        };
        let instance = Self::new_shell(prototype);
        <T as EmitOwn>::emit_own(&instance, args, ctx)?;
        Ok(instance)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        T::define_members(class)
    }
}

impl<T: ExtendLayer> Extended<T> {
    /// This class' registered prototype (Rust-side entry, no new_target).
    fn registered_prototype(ctx: &mut Context) -> JsResult<JsObject> {
        Ok(ctx
            .get_global_class::<Self>()
            .ok_or_else(|| crate::shared::native_error!(typ, "{} not registered", T::CLASS_NAME))?
            .prototype())
    }

    /// Build an instance with the prototype attached and an empty
    /// `OwnDataRegistry` (slots filled by `emit_own` / `populate_chain`).
    fn new_shell(prototype: JsObject) -> JsObject {
        JsObject::from_proto_and_data(prototype, OwnDataRegistry::new::<T>())
    }

    /// Rust-side creation from args, mirroring the JS `new` path: attach the
    /// registry and fill the layers recursively via `emit_own`.
    /// For layers whose own data is buildable from the given args.
    #[inline]
    pub fn new_native(args: &[JsValue], ctx: &mut Context) -> JsResult<JsObject> {
        let prototype = Self::registered_prototype(ctx)?;
        let instance = Self::new_shell(prototype);
        <T as EmitOwn>::emit_own(&instance, args, ctx)?;
        Ok(instance)
    }

    /// Rust-side creation from an existing data chain: attach the registry
    /// and fill the slots recursively via `populate_chain`. The chain type
    /// `LayerChain<T>` has non-`Option` fields, so nothing is taken.
    /// For layers whose own data comes from Rust and cannot be built from
    /// args (e.g. DOM nodes carrying a node_id).
    #[inline]
    pub fn from_chain(chain: <T as EmitOwn>::Chain, ctx: &mut Context) -> JsResult<JsObject> {
        let prototype = Self::registered_prototype(ctx)?;
        let instance = Self::new_shell(prototype);
        <T as EmitOwn>::populate_chain(&instance, chain, ctx)?;
        Ok(instance)
    }
}

// ── link_prototype: derived from Extended<T> ─────────────────────────

/// Link `Extended<T>`'s prototype / constructor to the parent class.
/// Takes `Extended<T>` (e.g. `Node`, `Element`); the parent is derived from
/// `T::Parent`. `RootLayer` has no JS class — the link is a no-op.
pub fn link_prototype<E: ExtendedOf>(ctx: &mut Context) -> JsResult<()>
where
    <E::Layer as ExtendLayer>::Parent: HasClass,
{
    let Some(parent_proto_ctor) = <E::Layer as ExtendLayer>::Parent::class_handles(ctx)? else {
        return Ok(()); // The parent is RootLayer: no prototype chain to link.
    };
    let child_sc = ctx
        .realm()
        .get_class::<E>()
        .ok_or_else(|| crate::shared::native_error!(typ, "{} not registered", E::Layer::CLASS_NAME))?;
    let (parent_proto, parent_ctor) = parent_proto_ctor;
    child_sc.prototype().set_prototype(Some(parent_proto));
    child_sc.constructor().set_prototype(Some(parent_ctor));
    Ok(())
}

/// Associate `Extended<T>` with its layer type `T`, for `link_prototype`
/// derivation.
pub trait ExtendedOf: Class {
    type Layer: ExtendLayer;
}

impl<T: ExtendLayer> ExtendedOf for Extended<T> {
    type Layer = T;
}

/// Map an `EmitOwn` parent to its JS prototype / constructor handles.
/// `RootLayer` returns `None`.
pub trait HasClass {
    /// Returns `(prototype, constructor)`; `RootLayer` returns `None`.
    fn class_handles(ctx: &mut Context) -> JsResult<Option<(JsObject, JsObject)>>;
}

impl HasClass for RootLayer {
    fn class_handles(_ctx: &mut Context) -> JsResult<Option<(JsObject, JsObject)>> {
        Ok(None)
    }
}

impl<T: ExtendLayer> HasClass for T {
    fn class_handles(ctx: &mut Context) -> JsResult<Option<(JsObject, JsObject)>> {
        let sc = ctx
            .realm()
            .get_class::<Extended<T>>()
            .ok_or_else(|| crate::shared::native_error!(typ, "{} not registered", T::CLASS_NAME))?;
        Ok(Some((sc.prototype(), sc.constructor())))
    }
}
