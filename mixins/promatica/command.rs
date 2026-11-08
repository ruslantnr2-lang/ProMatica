// `/promatica ...` and `/hm ...`, run on the client before the server sees them.
fn promatica_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !*ran && game.native_on(crate::game::promatica::ID) && crate::game::promatica::command(game, command) {
        *ran = true;
    }
}
