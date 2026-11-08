// The world is drawn: the overlay goes on top (see bridge.rs).
fn promatica_world_drawn(world: &mut crate::gfx::native_render::WorldDrawn<'_>) {
    crate::gfx::native_render::promatica_bridge::draw(world);
}
