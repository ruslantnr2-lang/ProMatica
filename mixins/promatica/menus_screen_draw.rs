// horizon-menus: draws promatica's menu screen, in the Debug Options screen's place.
fn promatica_screen_draw(game: &mut crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::promatica_menus::draw(game, batch, crate::game::promatica::screen);
}
