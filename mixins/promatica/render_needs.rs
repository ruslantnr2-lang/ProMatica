// The overlay hides behind terrain by the world's depth: keep it for `world_drawn`.
fn promatica_render_needs(needs: &mut crate::gfx::native_render::RenderNeeds) {
    needs.world_depth = true;
}
