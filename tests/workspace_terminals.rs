use std::path::Path;

use opencode_multiplexer::terminal::shells::ShellManager;

#[test]
fn shell_manager_spawns_resizes_and_closes_a_shell_pane() {
    let mut shells = ShellManager::default();
    let id = shells.spawn(Path::new("/tmp"), 12, 40).unwrap();

    assert!(shells.is_alive(id));
    shells.resize(id, 20, 70).unwrap();
    shells.close(id).unwrap();
    assert!(!shells.is_alive(id));
}
