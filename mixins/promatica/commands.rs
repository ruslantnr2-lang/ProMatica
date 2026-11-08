// The command names, so chat colours and completes them.
fn promatica_commands(commands: &mut Vec<&'static str>) {
    crate::game::promatica::commands(commands);
}
