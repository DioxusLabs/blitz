use dioxus_native::prelude::Modifiers;


#[cfg(target_os = "macos")]
pub fn is_shortcut_mod(mods: Modifiers) -> bool {
  mods.meta()
}

#[cfg(not(target_os = "macos"))]
pub fn is_shortcut_mod(mods: Modifiers) -> bool {
  mods.ctrl()
}