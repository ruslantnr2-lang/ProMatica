// The mod's entry and keys in Options > Mods.
fn promatica_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::promatica::listed());
}
