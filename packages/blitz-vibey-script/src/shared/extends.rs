//! The `Extended<T>` scheme: real `super` hooks — calling `sup` triggers the
//! parent's recursive construction.
//!
//! - `T: ExtendLayer` is not a Boa class, just a native data container
//!   describing one layer of an inheritance hierarchy.
//! - `Extended<T>` itself implements `Class`; the data slot holds only a ZST
//!   shell, all own data goes into Symbol slots.
//! - `build(args, ctx, sup)`: the subclass calls `sup.call(parent_args, ctx)`
//!   at the point where ES semantics place `super`. At that moment the
//!   parent's `emit_own` actually runs on the instance and writes the
//!   parent's own block; code after the call reads the parent's data via the
//!   `SuperDone` token.
//! - "Forgetting to call super" is enforced by the type system: `build` must
//!   return `Constructed<Self>`, which can only be assembled from the
//!   `SuperDone` produced by `sup.call(...)`.
//! - Instantiation entry points:
//!   - JS `new X()`: `Class::construct` builds the shell, recursive
//!     `emit_own` creates each layer's own block from the args.
//!   - Rust (args-buildable layers): `Extended::new_native(args, ctx)`.
//!   - Rust (existing data chain): `Extended::from_chain(chain, ctx)`; the
//!     `LayerChain` fields are non-`Option` and are moved into place by
//!     recursive `populate_chain`.
//! - Own blocks are read and written through `with_own` (zero-copy read) and
//!   `with_own_mut` (in-place write).

use boa_engine::class::{Class, ClassBuilder};
use boa_engine::object::{NativeObject, PROTOTYPE};
use boa_engine::symbol::JsSymbol;
use boa_engine::{js_string, Context, Finalize, JsData, JsObject, JsResult, JsValue, Trace};
use std::marker::PhantomData;

// ── Own-block Symbol key, wrapper and accessors ──────────────────────

/// Symbol key of an own block, derived from `T::CLASS_NAME`.
/// `Symbol("{CLASS_NAME}:own")` is cached in a thread-local so that symbols
/// of the same name stay unique within a thread. `JsSymbol` is `!Send` (a
/// Boa GC handle) and lives as long as the `Context`.
#[inline]
pub fn own_symbol<T: ExtendLayer>() -> JsSymbol {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static CACHE: RefCell<HashMap<&'static str, JsSymbol>> = RefCell::new(HashMap::new());
    }
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(T::CLASS_NAME)
            .or_insert_with(|| JsSymbol::new(Some(js_string!(format!("{}:own", T::CLASS_NAME)))).unwrap())
            .clone()
    })
}

/// Wrap own-block data into a prototype-less `JsObject`.
#[inline]
pub fn wrap_own<T: ExtendLayer>(data: T) -> JsObject {
    JsObject::from_proto_and_data(None, data)
}

/// Read an own block through a callback, zero-copy.
///
/// The `JsValue` returned by `obj.get(sym)` keeps the own `JsObject` alive
/// for the duration of the call; `downcast_ref` borrows from it and the
/// callback receives `&T` within that borrow, so the block is never
/// deep-copied.
#[inline]
pub fn with_own<T, R>(obj: &JsObject, ctx: &mut Context, f: impl FnOnce(&T) -> R) -> JsResult<R>
where
    T: ExtendLayer,
{
    let own_val = obj.get(own_symbol::<T>(), ctx)?;
    let own_obj = own_val
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "own block missing"))?;
    let data = own_obj
        .downcast_ref::<T>()
        .ok_or_else(|| crate::shared::native_error!(typ, "own block type mismatch"))?;
    Ok(f(&data))
}

/// Write to an own block in place through a callback.
///
/// `downcast_mut` yields `&mut T`; the callback mutates the block directly.
#[inline]
pub fn with_own_mut<T, R>(obj: &JsObject, ctx: &mut Context, f: impl FnOnce(&mut T) -> R) -> JsResult<R>
where
    T: ExtendLayer,
{
    let own_val = obj.get(own_symbol::<T>(), ctx)?;
    let own_obj = own_val
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "own block missing"))?;
    let mut data = own_obj
        .downcast_mut::<T>()
        .ok_or_else(|| crate::shared::native_error!(typ, "own block type mismatch"))?;
    Ok(f(&mut *data))
}

// ── Super handle + Constructed token ─────────────────────────────────

/// Token proving that a `super` call has completed. Only `Super::call` can
/// produce one.
///
/// Carries the instance reference: after `super`, the subclass reads the
/// parent's data (already written onto the instance) through it, mirroring
/// the ES rule that `this` is only usable after `super`. The field is
/// private, so a token cannot be forged to bypass `sup.call`; the type
/// system enforces super-before-return.
pub struct SuperDone<'a> {
    this: &'a JsObject,
}

impl<'a> SuperDone<'a> {
    /// The instance reference, valid after `super`.
    pub fn this(&self) -> &JsObject {
        self.this
    }
}

/// The return value of `build`. Can only be assembled from a `SuperDone`
/// plus the layer's own data, which forces `super` to precede the return.
pub struct Constructed<T> {
    own: T,
}

impl<T> Constructed<T> {
    pub fn new(_done: SuperDone<'_>, own: T) -> Self {
        Constructed { own }
    }
}

/// Handle to the parent constructor. Holds the instance; `call` triggers the
/// parent `P::emit_own` recursive construction.
pub struct Super<'a, P: EmitOwn> {
    instance: &'a JsObject,
    _parent: PhantomData<fn() -> P>,
}

impl<'a, P: EmitOwn> Super<'a, P> {
    /// Run the parent's recursive construction (writing the parent's own
    /// block onto the instance) and return the completion token carrying the
    /// instance.
    #[inline]
    pub fn call(self, parent_args: &[JsValue], ctx: &mut Context) -> JsResult<SuperDone<'a>> {
        P::emit_own(self.instance, parent_args, ctx)?;
        Ok(SuperDone { this: self.instance })
    }
}

// ── The ExtendLayer trait ────────────────────────────────────────────

/// One layer of an inheritance hierarchy: the layer's own data plus its
/// position in the prototype chain (`Parent`, `CLASS_NAME`).
pub trait ExtendLayer: Sized + NativeObject + Clone + 'static {
    type Parent: EmitOwn + 'static;
    const CLASS_NAME: &'static str;

    /// Construct this layer. Call `sup.call(parent_args, ctx)` where ES
    /// semantics place `super`, and assemble the returned `SuperDone` into
    /// the `Constructed`; code order around the call mirrors the ES sequence.
    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>>;

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()>;
}

// ── EmitOwn: recursive driver ────────────────────────────────────────

pub trait EmitOwn {
    /// JS `new` path: create each layer's own block from `args` and write it
    /// onto the instance.
    fn emit_own(instance: &JsObject, args: &[JsValue], ctx: &mut Context) -> JsResult<()>;

    /// Chain type for the Rust data path; fields are non-`Option`.
    /// `RootLayer::Chain = ()`, `T::Chain = LayerChain<T>`.
    type Chain;

    /// Write an existing data chain onto the instance, deconstructing it by
    /// move.
    fn populate_chain(instance: &JsObject, chain: Self::Chain, ctx: &mut Context) -> JsResult<()>;
}

/// Root terminator: no parent, no own block.
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

/// Node of a Rust-side data chain: this layer's own block plus the parent
/// chain. Fields are non-`Option` and consumed by move.
pub struct LayerChain<T: ExtendLayer> {
    pub parent: <T::Parent as EmitOwn>::Chain,
    pub own: T,
}

impl<T: ExtendLayer> EmitOwn for T {
    type Chain = LayerChain<T>;

    #[inline]
    fn emit_own(instance: &JsObject, args: &[JsValue], ctx: &mut Context) -> JsResult<()> {
        // Hand the super handle to `build`; the parent construction is
        // triggered by `build` calling `sup.call`.
        let sup = Super::<T::Parent> { instance, _parent: PhantomData };
        let constructed = T::build(args, ctx, sup)?;
        // By the time `build` returns, the parent is constructed (guaranteed
        // by `SuperDone`); write this layer's own block now.
        let own_obj = wrap_own(constructed.own);
        instance.set(own_symbol::<T>(), own_obj, true, ctx)?;
        Ok(())
    }

    #[inline]
    fn populate_chain(instance: &JsObject, chain: LayerChain<T>, ctx: &mut Context) -> JsResult<()> {
        // Parent chain first (ES super sequence).
        T::Parent::populate_chain(instance, chain.parent, ctx)?;
        let own_obj = wrap_own(chain.own);
        instance.set(own_symbol::<T>(), own_obj, true, ctx)?;
        Ok(())
    }
}

// ── Extended<T> + Class impl (direct Symbol-slot writes) ─────────────

/// `Class` shell for a layer: the data slot holds only this ZST, all own
/// data lives in Symbol slots (written by `construct`).
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct Extended<T: ExtendLayer> {
    _marker: PhantomData<fn() -> T>,
}

impl<T: ExtendLayer> Extended<T> {
    fn shell() -> Self {
        Extended { _marker: PhantomData }
    }
}

impl<T> Class for Extended<T>
where
    T: ExtendLayer + 'static,
{
    const NAME: &'static str = T::CLASS_NAME;

    fn data_constructor(_nt: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> JsResult<Self> {
        Ok(Extended::shell())
    }

    /// JS `new X()` path: resolve the prototype, build the shell, then run
    /// recursive `emit_own` to write the Symbol slots.
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
    /// The prototype this class was registered with (Rust-side entry points
    /// have no `new_target` to resolve it from).
    fn registered_prototype(ctx: &mut Context) -> JsResult<JsObject> {
        Ok(ctx
            .get_global_class::<Self>()
            .ok_or_else(|| crate::shared::native_error!(typ, "{} not registered", T::CLASS_NAME))?
            .prototype())
    }

    /// Build an instance with the prototype attached and an empty data slot;
    /// the own blocks are not written yet.
    fn new_shell(prototype: JsObject) -> JsObject {
        JsObject::from_proto_and_data(prototype, Extended::<T>::shell())
    }

    /// Rust-side construction from arguments, taking the same path as JS
    /// `new`: build the shell, then run recursive `emit_own`.
    #[inline]
    pub fn new_native(args: &[JsValue], ctx: &mut Context) -> JsResult<JsObject> {
        let prototype = Self::registered_prototype(ctx)?;
        let instance = Self::new_shell(prototype);
        <T as EmitOwn>::emit_own(&instance, args, ctx)?;
        Ok(instance)
    }

    /// Rust-side construction from an existing data chain: build the shell,
    /// then run recursive `populate_chain`.
    #[inline]
    pub fn from_chain(chain: <T as EmitOwn>::Chain, ctx: &mut Context) -> JsResult<JsObject> {
        let prototype = Self::registered_prototype(ctx)?;
        let instance = Self::new_shell(prototype);
        <T as EmitOwn>::populate_chain(&instance, chain, ctx)?;
        Ok(instance)
    }
}

// ── link_prototype ───────────────────────────────────────────────────

/// Link the prototype/constructor of `Extended<T>` to its parent class,
/// derived from `T::Parent`. `RootLayer` has no JS class, so linking is a
/// no-op there.
pub fn link_prototype<E: ExtendedOf>(ctx: &mut Context) -> JsResult<()>
where
    <E::Layer as ExtendLayer>::Parent: HasClass,
{
    let Some(parent_proto_ctor) = <E::Layer as ExtendLayer>::Parent::class_handles(ctx)? else {
        return Ok(()); // RootLayer: nothing to link.
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

/// Associates an `Extended<T>` with its layer type `T`, for
/// `link_prototype` to derive from.
pub trait ExtendedOf: Class {
    type Layer: ExtendLayer;
}

impl<T: ExtendLayer> ExtendedOf for Extended<T> {
    type Layer = T;
}

/// Maps an `EmitOwn` parent to the JS prototype/constructor handles of its
/// class. `RootLayer` yields `None`.
pub trait HasClass {
    /// Returns `(prototype, constructor)`.
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
