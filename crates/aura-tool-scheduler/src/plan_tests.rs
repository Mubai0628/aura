use super::*;

/// A refused reconcile no longer leaves the ordinary calls of its batch not
/// executed; what depends on the current task still waits for it.
#[test]
fn only_calls_that_depend_on_the_current_task_wait_for_a_reconcile() {
    let ids = (0..6).map(|_| ToolCallId::new()).collect::<Vec<_>>();
    let calls = [
        (ids[0], "task_lineage.reconcile"),
        (ids[1], "fs.read"),
        (ids[2], "memory.search"),
        (ids[3], "shell.powershell"),
        (ids[4], "todo.replace"),
        (ids[5], "normal.complete"),
    ];
    let dependencies = derive_control_dependencies(&calls);
    for ordinary in &ids[1..=3] {
        assert!(
            dependencies[ordinary].is_empty(),
            "ordinary tools run whatever the reconcile's outcome"
        );
    }
    assert_eq!(dependencies[&ids[4]], [ids[0]]);
    assert_eq!(dependencies[&ids[5]], [ids[0]]);
    assert!(dependencies[&ids[0]].is_empty());
}
