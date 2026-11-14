use super::branch_title;

#[test]
fn a_branch_is_titled_after_its_source_within_the_limit() {
    assert_eq!(branch_title("修复登录"), "分支：修复登录");
    assert_eq!(branch_title("  "), "分支：新会话");
    let long = "长".repeat(300);
    assert_eq!(branch_title(&long).chars().count(), 200);
}
