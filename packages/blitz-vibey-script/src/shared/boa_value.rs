macro_rules! as_object {
    ($val:expr, $et:ident, $fmt:literal $(, $args:expr)*) => {
        $val.as_object().ok_or_else(|| $crate::shared::native_error!($et, $fmt $(, $args)*))
    };
}
pub(crate) use as_object;
